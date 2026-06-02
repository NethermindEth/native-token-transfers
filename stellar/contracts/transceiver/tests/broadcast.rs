#![cfg(test)]

mod common;
#[path = "common/events.rs"]
mod events;

use common::setup_transceiver;
use events::posted_payload;
use soroban_ntt_client::{
    address_to_bytes32, TransceiverError, WormholeTransceiverInfo, WormholeTransceiverRegistration,
};
use soroban_sdk::{BytesN, Env};

const DEST_CHAIN: u32 = 6;
const TOO_LARGE: u32 = u16::MAX as u32 + 1;
const PEER: [u8; 32] = [0xAA; 32];

/// `broadcast_id` wires each manager query into the right `WormholeTransceiverInfo`
/// slot — mode, token (distinct from the manager address), decimals — and posts
/// it. Catches a swapped field, a dropped query, or a short-circuit that never
/// posts. The byte encoding itself is soroban-ntt-client's concern.
#[test]
fn broadcast_id_posts_typed_envelope() {
    let env = Env::default();
    let (_, _, transceiver, manager, core) = setup_transceiver(&env);

    transceiver.broadcast_id();
    let posted = posted_payload(&env, &core.address);

    let expected = WormholeTransceiverInfo {
        manager_address: address_to_bytes32(&manager.address),
        manager_mode: manager.get_mode(),
        token_address: address_to_bytes32(&manager.get_token()),
        token_decimals: manager.token_decimals() as u8,
    }
    .to_bytes(&env);
    assert_eq!(posted, expected);
}

/// When a manager query (`get_mode` / `get_token` / `token_decimals`) fails,
/// `broadcast_id` returns `ManagerQueryFailed` rather than posting a malformed
/// info message the Accountant would record.
#[test]
fn broadcast_id_propagates_manager_query_failure() {
    let env = Env::default();
    let (_, _, transceiver, manager, _) = setup_transceiver(&env);

    let mut cfg = manager.config();
    cfg.fail_query = true;
    manager.configure(&cfg);

    assert_eq!(
        transceiver.try_broadcast_id(),
        Err(Ok(TransceiverError::ManagerQueryFailed))
    );
}

/// `broadcast_peer` wires the chain id (u16) and the registered peer's address
/// into the `WormholeTransceiverRegistration` and posts it — catches a field
/// taken from the wrong source or a chain-id truncation.
#[test]
fn broadcast_peer_posts_registration_envelope() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, core) = setup_transceiver(&env);
    let emitter = BytesN::from_array(&env, &PEER);
    transceiver.set_peer(&DEST_CHAIN, &emitter);

    transceiver.broadcast_peer(&DEST_CHAIN);
    let posted = posted_payload(&env, &core.address);

    let expected = WormholeTransceiverRegistration {
        chain_id: DEST_CHAIN as u16,
        transceiver_address: emitter,
    }
    .to_bytes(&env);
    assert_eq!(posted, expected);
}

/// `broadcast_peer` rejects garbage a permissionless caller could otherwise post
/// to the Accountant: an out-of-range chain → `ChainIdTooLarge`, and a chain with
/// no registered peer → `PeerNotFound`.
#[test]
fn broadcast_peer_rejects_invalid_chain() {
    let env = Env::default();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);

    assert_eq!(
        transceiver.try_broadcast_peer(&TOO_LARGE),
        Err(Ok(TransceiverError::ChainIdTooLarge))
    );
    assert_eq!(
        transceiver.try_broadcast_peer(&DEST_CHAIN),
        Err(Ok(TransceiverError::PeerNotFound))
    );
}

/// The `#[when_not_paused]` gate covers both broadcasts: while paused, each
/// returns `EnforcedPause` (host error 1000). They share the gate and would
/// regress together, so one test asserts both.
#[test]
fn broadcasts_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, transceiver, _, _) = setup_transceiver(&env);
    transceiver.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER));
    transceiver.pause(&owner);

    assert_eq!(
        transceiver.try_broadcast_id(),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
    assert_eq!(
        transceiver.try_broadcast_peer(&DEST_CHAIN),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}
