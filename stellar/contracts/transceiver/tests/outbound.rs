#![cfg(test)]

mod common;

use common::setup;
use common::wormhole::MockWormholeCoreClient;
use soroban_ntt_client::{MessageSent, TransceiverError, TransceiverMessage};
use soroban_sdk::{
    testutils::Events as _, Bytes, BytesN, Env, Event,
};

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const RECIPIENT_MANAGER: [u8; 32] = [0xBB; 32];

/// `send_message` posts the on-the-wire `TransceiverMessage` envelope every
/// receiving peer parses — `source_manager` (this transceiver's manager id),
/// `recipient_manager` and `manager_payload` (the args), empty
/// `transceiver_payload` — and emits `MessageSent`. The event is built from the
/// raw args, so the decoded post is asserted separately to catch a mis-assembled
/// envelope.
#[test]
fn send_message_posts_envelope_and_emits() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));

    let recipient_manager = BytesN::from_array(&env, &RECIPIENT_MANAGER);
    let manager_payload = Bytes::from_array(&env, &[1, 2, 3, 4]);
    t.send_message(&PEER_CHAIN, &recipient_manager, &manager_payload);

    assert_eq!(
        env.events().all().filter_by_contract(&t.address),
        std::vec![MessageSent {
            recipient_chain: PEER_CHAIN,
            recipient_manager: recipient_manager.clone(),
            manager_payload: manager_payload.clone(),
        }
        .to_xdr(&env, &t.address)]
    );

    let envelope = TransceiverMessage::from_bytes(&env, &core.last_posted().unwrap().payload).unwrap();
    assert_eq!(envelope.source_manager, t.get_manager_id());
    assert_eq!(envelope.recipient_manager, recipient_manager);
    assert_eq!(envelope.manager_payload, manager_payload);
    assert_eq!(envelope.transceiver_payload, Bytes::new(&env));
}

/// Only the registered manager can dispatch outbound. Without the gate any
/// caller could post forged VAAs through the manager's wormhole emitter.
#[test]
fn send_message_requires_manager_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));

    env.mock_auths(&[]); // withhold the manager's authorization
    assert!(t
        .try_send_message(&PEER_CHAIN, &BytesN::from_array(&env, &RECIPIENT_MANAGER), &Bytes::new(&env))
        .is_err());
}

/// No peer for `recipient_chain` → `PeerNotFound`, so the contract never posts a
/// VAA the destination cannot authenticate.
#[test]
fn send_message_rejects_unregistered_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);

    assert_eq!(
        t.try_send_message(&PEER_CHAIN, &BytesN::from_array(&env, &RECIPIENT_MANAGER), &Bytes::new(&env)),
        Err(Ok(TransceiverError::PeerNotFound))
    );
}

/// A disabled peer → `PeerDisabled`: the `set_peer_enabled(false)` kill-switch
/// must take effect on outbound so the owner can cut off a compromised peer.
#[test]
fn send_message_rejects_disabled_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    t.set_peer_enabled(&PEER_CHAIN, &false);

    assert_eq!(
        t.try_send_message(&PEER_CHAIN, &BytesN::from_array(&env, &RECIPIENT_MANAGER), &Bytes::new(&env)),
        Err(Ok(TransceiverError::PeerDisabled))
    );
}

/// When the wormhole core rejects `post_message`, the contract surfaces
/// `WormholePostFailed` rather than reporting success — otherwise the manager
/// believes a message went out when it didn't.
#[test]
fn send_message_propagates_post_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    core.fail_post(&true);

    assert_eq!(
        t.try_send_message(&PEER_CHAIN, &BytesN::from_array(&env, &RECIPIENT_MANAGER), &Bytes::new(&env)),
        Err(Ok(TransceiverError::WormholePostFailed))
    );
}

/// The `#[when_not_paused]` gate blocks outbound while paused (`EnforcedPause`,
/// surfaced as host error 1000), so the owner can stop dispatch during an
/// incident.
#[test]
fn send_message_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    t.pause(&t.get_owner().unwrap());

    assert_eq!(
        t.try_send_message(&PEER_CHAIN, &BytesN::from_array(&env, &RECIPIENT_MANAGER), &Bytes::new(&env)),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}
