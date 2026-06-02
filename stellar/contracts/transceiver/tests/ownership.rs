#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;
#[path = "common/vaa.rs"]
mod vaa;

use common::setup_transceiver;
use messages::build_transceiver_payload;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};
use vaa::signed_inbound_vaa;

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
    let (owner, _, transceiver, _, _) = setup_transceiver(&env);
    let new_owner = Address::generate(&env);

    transceiver.transfer_ownership(&new_owner, &1000);
    assert_eq!(transceiver.get_owner(), Some(owner)); // pending, not yet accepted

    transceiver.accept_ownership();
    assert_eq!(transceiver.get_owner(), Some(new_owner.clone()));

    transceiver.renounce_ownership();
    assert_eq!(transceiver.get_owner(), None);
}

/// `unpause` clears the gate for every entrypoint: after pause then unpause, both
/// outbound `send_message` and inbound `receive_message` work again. Catches a
/// sticky pause flag or an unpause that only frees one direction.
#[test]
fn unpause_restores_io() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, manager_id, transceiver, _, _) = setup_transceiver(&env);
    let peer = BytesN::from_array(&env, &PEER_EMITTER);
    transceiver.set_peer(&PEER_CHAIN, &peer);

    transceiver.pause(&owner);
    transceiver.unpause(&owner);
    assert!(!transceiver.paused());

    transceiver.send_message(&PEER_CHAIN, &BytesN::from_array(&env, &[0xBB; 32]), &Bytes::new(&env));

    let payload = build_transceiver_payload(
        &env,
        &BytesN::from_array(&env, &SOURCE_MANAGER),
        &manager_id,
        &Bytes::new(&env),
    );
    transceiver.receive_message(&signed_inbound_vaa(&env, PEER_CHAIN as u16, &peer, 1, &payload));
    assert!(transceiver.is_vaa_consumed(&PEER_CHAIN, &peer, &1));
}

/// Every owner-only entrypoint is gated: with the owner's authorization withheld,
/// `set_peer`, `set_peer_enabled`, `pause`, `unpause`, and `upgrade` are each
/// rejected.
#[test]
fn privileged_setters_require_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, transceiver, _, _) = setup_transceiver(&env);
    let emitter = BytesN::from_array(&env, &[1u8; 32]);
    let hash = BytesN::from_array(&env, &[2u8; 32]);

    env.mock_auths(&[]); // owner no longer authorizes anything
    assert!(transceiver.try_set_peer(&3, &emitter).is_err());
    assert!(transceiver.try_set_peer_enabled(&3, &true).is_err());
    assert!(transceiver.try_pause(&owner).is_err());
    assert!(transceiver.try_unpause(&owner).is_err());
    assert!(transceiver.try_upgrade(&hash).is_err());
}
