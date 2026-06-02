#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;

use common::manager::MockNttManagerClient;
use common::setup;
use common::wormhole::MockWormholeCoreClient;
use messages::build_transceiver_payload;
use soroban_ntt_client::{MessageReceived, TransceiverError};
use soroban_sdk::{testutils::Events as _, Bytes, BytesN, Env, Event};
use stellar_ntt_transceiver::TransceiverContractClient;

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const SOURCE_MANAGER: [u8; 32] = [0x11; 32];

/// `setup` with auth mocked and the standard peer registered. Returns the
/// transceiver, the core mock to configure inbound VAAs on, and the derived
/// manager id.
fn setup_with_peer<'a>(
    env: &'a Env,
) -> (TransceiverContractClient<'a>, MockWormholeCoreClient<'a>, BytesN<32>) {
    env.mock_all_auths();
    let t = setup(env);
    let core = MockWormholeCoreClient::new(env, &t.get_wormhole_core());
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(env, &PEER_EMITTER));
    let manager_id = t.get_manager_id();
    (t, core, manager_id)
}

/// Configures the core to return a VAA from the standard peer carrying a
/// transceiver message from `SOURCE_MANAGER` to `recipient_manager`.
fn arrives(env: &Env, core: &MockWormholeCoreClient, recipient_manager: &BytesN<32>, sequence: u64, manager_payload: &Bytes) {
    core.set_vaa(
        &PEER_CHAIN,
        &BytesN::from_array(env, &PEER_EMITTER),
        &sequence,
        &build_transceiver_payload(env, &BytesN::from_array(env, &SOURCE_MANAGER), recipient_manager, manager_payload),
    );
}

/// The whole inbound pipeline: verify → peer check → manager-id check → forward →
/// replay-flag → emit. Asserts the forwarded fields the mock manager recorded,
/// the typed `MessageReceived` event, and the consumed flag. Any break shows here.
#[test]
fn receive_message_forwards_and_emits_and_marks_consumed() {
    let env = Env::default();
    let (t, core, manager_id) = setup_with_peer(&env);
    let manager = MockNttManagerClient::new(&env, &t.get_manager());
    let peer = BytesN::from_array(&env, &PEER_EMITTER);

    let manager_payload = Bytes::from_array(&env, &[1, 2, 3, 4]);
    arrives(&env, &core, &manager_id, 7, &manager_payload);
    t.receive_message(&Bytes::new(&env));

    assert_eq!(
        env.events().all().filter_by_contract(&t.address),
        std::vec![MessageReceived {
            emitter_chain: PEER_CHAIN,
            emitter_address: peer.clone(),
            sequence: 7,
        }
        .to_xdr(&env, &t.address)]
    );

    let last = manager.last_attestation().unwrap();
    assert_eq!(last.transceiver, t.address);
    assert_eq!(last.source_chain, PEER_CHAIN);
    assert_eq!(last.source_manager, BytesN::from_array(&env, &SOURCE_MANAGER));
    assert_eq!(last.payload, manager_payload);
    assert!(t.is_vaa_consumed(&PEER_CHAIN, &peer, &7));
}

/// A VAA the core rejects on verification → `WormholeVerificationFailed`. (Real
/// signature verification is exercised in the IT suite; here the mapping is.)
#[test]
fn receive_message_rejects_failed_verification() {
    let env = Env::default();
    let (t, core, _) = setup_with_peer(&env);
    core.fail_verify(&true);

    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Ok(TransceiverError::WormholeVerificationFailed))
    );
}

/// A verified VAA whose emitter is not the registered peer for its chain →
/// `UnexpectedEmitter`. Without this, any compromised emitter on the source chain
/// could spoof the peer.
#[test]
fn receive_message_rejects_unexpected_emitter() {
    let env = Env::default();
    let (t, core, manager_id) = setup_with_peer(&env);

    let imposter = BytesN::from_array(&env, &[0xDD; 32]);
    core.set_vaa(
        &PEER_CHAIN,
        &imposter,
        &7,
        &build_transceiver_payload(&env, &BytesN::from_array(&env, &SOURCE_MANAGER), &manager_id, &Bytes::new(&env)),
    );

    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Ok(TransceiverError::UnexpectedEmitter))
    );
}

/// No valid peer for the message: an unconfigured source chain → `PeerNotFound`,
/// and a disabled peer → `PeerDisabled`.
#[test]
fn receive_message_rejects_unknown_or_disabled_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    let manager_id = t.get_manager_id();

    arrives(&env, &core, &manager_id, 7, &Bytes::new(&env));
    assert_eq!(t.try_receive_message(&Bytes::new(&env)), Err(Ok(TransceiverError::PeerNotFound)));

    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    t.set_peer_enabled(&PEER_CHAIN, &false);
    arrives(&env, &core, &manager_id, 8, &Bytes::new(&env));
    assert_eq!(t.try_receive_message(&Bytes::new(&env)), Err(Ok(TransceiverError::PeerDisabled)));
}

/// A VAA whose decoded `recipient_manager` isn't this transceiver's manager id →
/// `UnexpectedRecipientManager`. Prevents transceiver A from consuming a message
/// addressed to manager B.
#[test]
fn receive_message_rejects_wrong_recipient_manager() {
    let env = Env::default();
    let (t, core, _) = setup_with_peer(&env);

    let wrong_manager = BytesN::from_array(&env, &[0x99; 32]);
    arrives(&env, &core, &wrong_manager, 7, &Bytes::new(&env));
    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Ok(TransceiverError::UnexpectedRecipientManager))
    );
}

/// The same `(emitter_chain, emitter_address, sequence)` consumed twice →
/// `ReplayDetected`. Double-spend protection — the core of inbound safety.
#[test]
fn receive_message_replay_protected() {
    let env = Env::default();
    let (t, core, manager_id) = setup_with_peer(&env);
    let peer = BytesN::from_array(&env, &PEER_EMITTER);

    arrives(&env, &core, &manager_id, 7, &Bytes::new(&env));
    t.receive_message(&Bytes::new(&env));
    assert!(t.is_vaa_consumed(&PEER_CHAIN, &peer, &7));

    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Ok(TransceiverError::ReplayDetected))
    );
}

/// When the manager rejects the forwarded attestation, the transceiver surfaces
/// `ManagerRejectedMessage` and — because the reject reverts the call — does not
/// leave the VAA marked consumed, so it can be retried once the manager recovers.
#[test]
fn receive_message_propagates_manager_rejection() {
    let env = Env::default();
    let (t, core, manager_id) = setup_with_peer(&env);
    let peer = BytesN::from_array(&env, &PEER_EMITTER);
    MockNttManagerClient::new(&env, &t.get_manager()).fail_attestation(&true);

    arrives(&env, &core, &manager_id, 7, &Bytes::new(&env));
    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Ok(TransceiverError::ManagerRejectedMessage))
    );
    assert!(!t.is_vaa_consumed(&PEER_CHAIN, &peer, &7));
}

/// The `#[when_not_paused]` gate blocks inbound while paused (`EnforcedPause`,
/// host error 1000), so the kill-switch can't be bypassed on the inbound path.
#[test]
fn receive_message_blocked_when_paused() {
    let env = Env::default();
    let (t, core, manager_id) = setup_with_peer(&env);
    t.pause(&t.get_owner().unwrap());

    arrives(&env, &core, &manager_id, 7, &Bytes::new(&env));
    assert_eq!(
        t.try_receive_message(&Bytes::new(&env)),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}
