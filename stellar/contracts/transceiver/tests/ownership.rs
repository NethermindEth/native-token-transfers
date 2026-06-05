#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;

use common::setup;
use common::wormhole::MockWormholeCoreClient;
use messages::build_transceiver_payload;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const SOURCE_MANAGER: [u8; 32] = [0x11; 32];

/// OZ two-step ownership: the new owner is only pending until they accept (owner
/// unchanged), then ownership moves, and renouncing afterwards clears it.
/// Catches the two-step Ownable not being wired up on the transceiver at all.
#[test]
fn transfer_ownership_two_step_and_renounce() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let owner = t.get_owner().unwrap();
    let new_owner = Address::generate(&env);

    t.transfer_ownership(&new_owner, &1000);
    assert_eq!(t.get_owner(), Some(owner)); // pending, not yet accepted

    t.accept_ownership();
    assert_eq!(t.get_owner(), Some(new_owner.clone()));

    t.renounce_ownership();
    assert_eq!(t.get_owner(), None);
}

/// `unpause` clears the gate for every entrypoint: after pause then unpause, both
/// outbound `send_message` and inbound `receive_message` work again. Catches a
/// sticky pause flag or an unpause that only frees one direction.
#[test]
fn unpause_restores_io() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let core = MockWormholeCoreClient::new(&env, &t.get_wormhole_core());
    let owner = t.get_owner().unwrap();
    let peer = BytesN::from_array(&env, &PEER_EMITTER);
    t.set_peer(&PEER_CHAIN, &peer);

    t.pause(&owner);
    t.unpause(&owner);
    assert!(!t.paused());

    t.send_message(&PEER_CHAIN, &BytesN::from_array(&env, &[0xBB; 32]), &Bytes::new(&env));

    core.set_vaa(
        &PEER_CHAIN,
        &peer,
        &1,
        &build_transceiver_payload(&env, &BytesN::from_array(&env, &SOURCE_MANAGER), &t.get_manager_id(), &Bytes::new(&env)),
    );
    t.receive_message(&Bytes::new(&env));
    assert!(t.is_vaa_consumed(&PEER_CHAIN, &peer, &1));
}

/// Every owner-only entrypoint is gated: with the owner's authorization withheld,
/// `set_peer`, `set_peer_enabled`, `pause`, `unpause`, and `upgrade` are each
/// rejected.
#[test]
fn privileged_setters_require_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let owner = t.get_owner().unwrap();
    let emitter = BytesN::from_array(&env, &[1u8; 32]);
    let hash = BytesN::from_array(&env, &[2u8; 32]);

    env.mock_auths(&[]); // owner no longer authorizes anything
    assert!(t.try_set_peer(&3, &emitter).is_err());
    assert!(t.try_set_peer_enabled(&3, &true).is_err());
    assert!(t.try_pause(&owner).is_err());
    assert!(t.try_unpause(&owner).is_err());
    assert!(t.try_upgrade(&hash).is_err());
}
