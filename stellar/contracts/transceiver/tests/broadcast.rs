#![cfg(test)]

mod common;
#[path = "common/xlm.rs"]
mod xlm;

use common::manager::MockNttManagerClient;
use common::setup;
use common::wormhole::MockWormholeCoreClient;
use soroban_ntt_client::{
    TransceiverError, WormholeTransceiverInfo, WormholeTransceiverRegistration,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use wormhole_soroban_client::hash_address;
use xlm::deploy_xlm;

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const TOO_LARGE: u32 = u16::MAX as u32 + 1;

/// `broadcast_id` wires each manager query into the right `WormholeTransceiverInfo`
/// slot — mode, token (distinct from the manager address), decimals — and posts
/// it. Catches a swapped field, a dropped query, or a short-circuit that never
/// posts. The byte encoding itself is soroban-ntt-client's concern.
#[test]
fn broadcast_id_posts_typed_envelope() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    let manager = MockNttManagerClient::new(&env, &t.get_manager());

    t.broadcast_id(&Address::generate(&env));

    let expected = WormholeTransceiverInfo {
        manager_address: hash_address(&env, &t.get_manager()),
        manager_mode: manager.get_mode(),
        token_address: hash_address(&env, &manager.get_token()),
        token_decimals: manager.token_decimals() as u8,
    }
    .to_bytes(&env);
    assert_eq!(core.last_posted().unwrap().payload, expected);
}

/// When a manager query (`get_mode` / `get_token` / `token_decimals`) fails,
/// `broadcast_id` returns `ManagerQueryFailed` rather than posting a malformed
/// info message the Accountant would record.
#[test]
fn broadcast_id_propagates_manager_query_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    MockNttManagerClient::new(&env, &t.get_manager()).fail_query(&true);

    assert_eq!(
        t.try_broadcast_id(&Address::generate(&env)),
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
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    let emitter = BytesN::from_array(&env, &PEER_EMITTER);
    t.set_peer(&PEER_CHAIN, &emitter);

    t.broadcast_peer(&Address::generate(&env), &PEER_CHAIN);

    let expected = WormholeTransceiverRegistration {
        chain_id: PEER_CHAIN as u16,
        transceiver_address: emitter,
    }
    .to_bytes(&env);
    assert_eq!(core.last_posted().unwrap().payload, expected);
}

/// `broadcast_peer` rejects garbage a permissionless caller could otherwise post
/// to the Accountant: an out-of-range chain → `ChainIdTooLarge`, and a chain with
/// no registered peer → `PeerNotFound`.
#[test]
fn broadcast_peer_rejects_invalid_chain() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let payer = Address::generate(&env);

    assert_eq!(
        t.try_broadcast_peer(&payer, &TOO_LARGE),
        Err(Ok(TransceiverError::ChainIdTooLarge))
    );
    assert_eq!(
        t.try_broadcast_peer(&payer, &PEER_CHAIN),
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
    let t = setup(&env);
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    t.pause(&t.get_owner().unwrap());
    let payer = Address::generate(&env);

    assert_eq!(
        t.try_broadcast_id(&payer),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
    assert_eq!(
        t.try_broadcast_peer(&payer, &PEER_CHAIN),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}

/// With a core message fee set, each broadcast needs `payer`'s signature and
/// takes the fee from `payer`. The payer can't be the transceiver itself, so a
/// permissionless post never spends the transceiver's own XLM.
#[test]
fn broadcasts_charge_payer() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    t.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &PEER_EMITTER));
    core.set_fee(&100);
    let payer = Address::generate(&env);
    let xlm = deploy_xlm(&env, &payer, 1000);
    xlm.transfer(&payer, &t.address, &100);

    env.mock_auths(&[]);
    for unsigned in [&payer, &t.address] {
        assert!(t.try_broadcast_id(unsigned).is_err());
        assert!(t.try_broadcast_peer(unsigned, &PEER_CHAIN).is_err());
    }
    env.mock_all_auths();
    t.broadcast_id(&payer);
    t.broadcast_peer(&payer, &PEER_CHAIN);

    assert_eq!(xlm.balance(&payer), 700);
    assert_eq!(xlm.balance(&core.address), 200);
    assert_eq!(xlm.balance(&t.address), 100);
}
