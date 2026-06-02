#![cfg(test)]

mod common;
#[path = "common/events.rs"]
mod events;

use common::setup_transceiver;
use events::posted_payload;
use soroban_ntt_client::{MessageSent, TransceiverError, TransceiverMessage};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Bytes, BytesN, Env, Event,
};
use stellar_ntt_transceiver::{TransceiverContract, TransceiverContractClient};

const DEST_CHAIN: u32 = 6;
const PEER: [u8; 32] = [0xAA; 32];
const RECIPIENT_MANAGER: [u8; 32] = [0xBB; 32];

/// `send_message` posts the on-the-wire `TransceiverMessage` envelope every
/// receiving peer parses — `source_manager` (this transceiver's manager id),
/// `recipient_manager` and `manager_payload` (the args), empty
/// `transceiver_payload` — and emits `MessageSent` for indexers. Envelope drift
/// breaks every peer; the event is built from the raw args, so the decoded post
/// is asserted separately to catch a mis-assembled envelope.
#[test]
fn send_message_posts_envelope_and_emits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, manager_id, transceiver, _, core) = setup_transceiver(&env);
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));

    let recipient_manager = BytesN::from_array(&env, &RECIPIENT_MANAGER);
    let manager_payload = Bytes::from_array(&env, &[1, 2, 3, 4]);
    transceiver.send_message(&DEST_CHAIN, &recipient_manager, &manager_payload);

    assert_eq!(
        env.events().all().filter_by_contract(&transceiver.address),
        std::vec![MessageSent {
            recipient_chain: DEST_CHAIN,
            recipient_manager: recipient_manager.clone(),
            manager_payload: manager_payload.clone(),
        }
        .to_xdr(&env, &transceiver.address)]
    );

    let envelope = TransceiverMessage::from_bytes(&env, &posted_payload(&env, &core.address)).unwrap();
    assert_eq!(envelope.source_manager, manager_id);
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
    let (_, _, transceiver, _, _) = setup_transceiver(&env);
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));

    env.mock_auths(&[]); // withhold the manager's authorization
    assert!(transceiver
        .try_send_message(
            &DEST_CHAIN,
            &BytesN::from_array(&env, &RECIPIENT_MANAGER),
            &Bytes::new(&env),
        )
        .is_err());
}

/// No peer for `recipient_chain` → `PeerNotFound`, so the contract never posts a
/// VAA the destination cannot authenticate.
#[test]
fn send_message_rejects_unregistered_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);

    assert_eq!(
        transceiver.try_send_message(
            &DEST_CHAIN,
            &BytesN::from_array(&env, &RECIPIENT_MANAGER),
            &Bytes::new(&env),
        ),
        Err(Ok(TransceiverError::PeerNotFound))
    );
}

/// A disabled peer → `PeerDisabled`: the `set_peer_enabled(false)` kill-switch
/// must take effect on outbound so the owner can cut off a compromised peer.
#[test]
fn send_message_rejects_disabled_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));
    transceiver.set_peer_enabled(&DEST_CHAIN, &false);

    assert_eq!(
        transceiver.try_send_message(
            &DEST_CHAIN,
            &BytesN::from_array(&env, &RECIPIENT_MANAGER),
            &Bytes::new(&env),
        ),
        Err(Ok(TransceiverError::PeerDisabled))
    );
}

/// When the wormhole core rejects `post_message`, the contract surfaces
/// `WormholePostFailed` rather than reporting success — otherwise the manager
/// believes a message went out when it didn't. A transceiver pointed at a core
/// address that cannot service the call exercises the real failure.
#[test]
fn send_message_propagates_post_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, manager, _) = setup_transceiver(&env);
    let owner = Address::generate(&env);
    let broken_core = Address::generate(&env);
    let transceiver = TransceiverContractClient::new(
        &env,
        &env.register(TransceiverContract, (&owner, &manager.address, &broken_core)),
    );
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));

    assert_eq!(
        transceiver.try_send_message(
            &DEST_CHAIN,
            &BytesN::from_array(&env, &RECIPIENT_MANAGER),
            &Bytes::new(&env),
        ),
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
    let (owner, _, transceiver, _, _) = setup_transceiver(&env);
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));
    transceiver.pause(&owner);

    assert_eq!(
        transceiver.try_send_message(
            &DEST_CHAIN,
            &BytesN::from_array(&env, &RECIPIENT_MANAGER),
            &Bytes::new(&env),
        ),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}
