#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;
#[path = "common/vaa.rs"]
mod vaa;

use common::guardian::GUARDIAN_SECRET;
use common::manager::MockNttManagerClient;
use common::setup_transceiver;
use messages::build_transceiver_payload;
use soroban_ntt_client::{MessageReceived, TransceiverError};
use soroban_sdk::{testutils::Events as _, Bytes, BytesN, Env, Event};
use stellar_ntt_transceiver::TransceiverContractClient;
use vaa::{serialize_body, signed_inbound_vaa};
use vaa_test_signing::{assemble, sign};

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const SOURCE_MANAGER: [u8; 32] = [0x11; 32];

/// `setup_transceiver` with auth mocked and the standard peer registered.
/// Returns the manager id, the transceiver, the mock manager, and the peer
/// emitter bytes for inbound assertions.
fn setup_with_peer<'a>(
    env: &Env,
) -> (BytesN<32>, TransceiverContractClient<'a>, MockNttManagerClient<'a>, BytesN<32>) {
    env.mock_all_auths();
    let (_, manager_id, transceiver, manager, _) = setup_transceiver(env);
    let peer = BytesN::from_array(env, &PEER_EMITTER);
    transceiver.set_peer(&PEER_CHAIN, &peer);
    (manager_id, transceiver, manager, peer)
}

/// The transceiver-message envelope a peer VAA carries: from `SOURCE_MANAGER` to
/// `recipient_manager`, wrapping `manager_payload`.
fn envelope(env: &Env, recipient_manager: &BytesN<32>, manager_payload: &Bytes) -> Bytes {
    build_transceiver_payload(env, &BytesN::from_array(env, &SOURCE_MANAGER), recipient_manager, manager_payload)
}

/// A guardian-signed VAA from the registered peer at `sequence`.
fn peer_vaa(env: &Env, recipient_manager: &BytesN<32>, sequence: u64, manager_payload: &Bytes) -> Bytes {
    signed_inbound_vaa(
        env,
        PEER_CHAIN as u16,
        &BytesN::from_array(env, &PEER_EMITTER),
        sequence,
        &envelope(env, recipient_manager, manager_payload),
    )
}

/// A [`peer_vaa`] with one signature byte flipped, so the core's verification
/// rejects it.
fn tampered_peer_vaa(env: &Env, recipient_manager: &BytesN<32>, sequence: u64) -> Bytes {
    let body = serialize_body(
        env,
        PEER_CHAIN as u16,
        &BytesN::from_array(env, &PEER_EMITTER),
        sequence,
        &envelope(env, recipient_manager, &Bytes::new(env)),
    );
    let mut bad = sign(&body, &GUARDIAN_SECRET, 0);
    bad.sig[0] ^= 0xFF;
    Bytes::from_slice(env, &assemble(0, &[bad], &body))
}

/// The whole inbound pipeline: the real core verifies the VAA, the transceiver
/// checks the peer and recipient manager, forwards the decoded message, flips
/// the replay flag, and emits `MessageReceived`. Any break in the chain shows
/// here — it asserts the forwarded fields, the typed event, and the flag.
#[test]
fn receive_message_forwards_and_emits_and_marks_consumed() {
    let env = Env::default();
    let (manager_id, transceiver, manager, peer) = setup_with_peer(&env);

    let manager_payload = Bytes::from_array(&env, &[1, 2, 3, 4]);
    transceiver.receive_message(&peer_vaa(&env, &manager_id, 7, &manager_payload));

    assert_eq!(
        env.events().all().filter_by_contract(&transceiver.address),
        std::vec![MessageReceived {
            emitter_chain: PEER_CHAIN,
            emitter_address: peer.clone(),
            sequence: 7,
        }
        .to_xdr(&env, &transceiver.address)]
    );

    let last = manager.last_attestation().unwrap();
    assert_eq!(last.transceiver, transceiver.address);
    assert_eq!(last.source_chain, PEER_CHAIN);
    assert_eq!(last.source_manager, BytesN::from_array(&env, &SOURCE_MANAGER));
    assert_eq!(last.payload, manager_payload);
    assert!(transceiver.is_vaa_consumed(&PEER_CHAIN, &peer, &7));
}

/// A VAA with a tampered guardian signature is rejected by the real core with
/// `WormholeVerificationFailed`. Trusting an unverified VAA is the highest-
/// severity bug this contract could have.
#[test]
fn receive_message_rejects_failed_verification() {
    let env = Env::default();
    let (manager_id, transceiver, _, _) = setup_with_peer(&env);

    assert_eq!(
        transceiver.try_receive_message(&tampered_peer_vaa(&env, &manager_id, 7)),
        Err(Ok(TransceiverError::WormholeVerificationFailed))
    );
}

/// A VAA that verifies (same guardian) but whose emitter is not the registered
/// peer for its chain → `UnexpectedEmitter`. Without this, any compromised
/// emitter on the source chain could spoof the peer.
#[test]
fn receive_message_rejects_unexpected_emitter() {
    let env = Env::default();
    let (manager_id, transceiver, _, _) = setup_with_peer(&env);

    let imposter = BytesN::from_array(&env, &[0xDD; 32]);
    let vaa = signed_inbound_vaa(&env, PEER_CHAIN as u16, &imposter, 7, &envelope(&env, &manager_id, &Bytes::new(&env)));
    assert_eq!(
        transceiver.try_receive_message(&vaa),
        Err(Ok(TransceiverError::UnexpectedEmitter))
    );
}

/// No valid peer for the message: an unconfigured source chain → `PeerNotFound`,
/// and a disabled peer → `PeerDisabled`. Both are "no peer accepts this" and fold
/// into one test.
#[test]
fn receive_message_rejects_unknown_or_disabled_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, manager_id, transceiver, _, _) = setup_transceiver(&env);

    assert_eq!(
        transceiver.try_receive_message(&peer_vaa(&env, &manager_id, 7, &Bytes::new(&env))),
        Err(Ok(TransceiverError::PeerNotFound))
    );

    transceiver.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    transceiver.set_peer_enabled(&PEER_CHAIN, &false);
    assert_eq!(
        transceiver.try_receive_message(&peer_vaa(&env, &manager_id, 8, &Bytes::new(&env))),
        Err(Ok(TransceiverError::PeerDisabled))
    );
}

/// A VAA whose decoded `recipient_manager` isn't this transceiver's manager id →
/// `UnexpectedRecipientManager`. Prevents transceiver A from consuming a message
/// addressed to manager B.
#[test]
fn receive_message_rejects_wrong_recipient_manager() {
    let env = Env::default();
    let (_, transceiver, _, _) = setup_with_peer(&env);

    let wrong_manager = BytesN::from_array(&env, &[0x99; 32]);
    assert_eq!(
        transceiver.try_receive_message(&peer_vaa(&env, &wrong_manager, 7, &Bytes::new(&env))),
        Err(Ok(TransceiverError::UnexpectedRecipientManager))
    );
}

/// The same `(emitter_chain, emitter_address, sequence)` consumed twice →
/// `ReplayDetected`. Double-spend protection — the core of inbound safety.
#[test]
fn receive_message_replay_protected() {
    let env = Env::default();
    let (manager_id, transceiver, _, peer) = setup_with_peer(&env);

    let vaa = peer_vaa(&env, &manager_id, 7, &Bytes::new(&env));
    transceiver.receive_message(&vaa);
    assert!(transceiver.is_vaa_consumed(&PEER_CHAIN, &peer, &7));

    assert_eq!(
        transceiver.try_receive_message(&vaa),
        Err(Ok(TransceiverError::ReplayDetected))
    );
}

/// When the manager rejects the forwarded attestation, the transceiver surfaces
/// `ManagerRejectedMessage` and — because the reject reverts the call — does not
/// leave the VAA marked consumed, so it can be retried once the manager recovers.
#[test]
fn receive_message_propagates_manager_rejection() {
    let env = Env::default();
    let (manager_id, transceiver, manager, peer) = setup_with_peer(&env);

    let mut cfg = manager.config();
    cfg.fail_attestation = true;
    manager.configure(&cfg);

    assert_eq!(
        transceiver.try_receive_message(&peer_vaa(&env, &manager_id, 7, &Bytes::new(&env))),
        Err(Ok(TransceiverError::ManagerRejectedMessage))
    );
    assert!(!transceiver.is_vaa_consumed(&PEER_CHAIN, &peer, &7));
}

/// The `#[when_not_paused]` gate blocks inbound while paused (`EnforcedPause`,
/// host error 1000), so the kill-switch can't be bypassed on the inbound path.
#[test]
fn receive_message_blocked_when_paused() {
    let env = Env::default();
    let (manager_id, transceiver, _, _) = setup_with_peer(&env);
    transceiver.pause(&transceiver.get_owner().unwrap());

    assert_eq!(
        transceiver.try_receive_message(&peer_vaa(&env, &manager_id, 7, &Bytes::new(&env))),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}
