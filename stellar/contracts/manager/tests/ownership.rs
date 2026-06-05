#![cfg(test)]

mod common;

use common::setup_manager;
use soroban_ntt_client::{Mode, NttManagerError};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env,
};

const OUR_CHAIN: u32 = 2;

/// Two-step ownership transfer: the new owner is only pending (owner unchanged)
/// until they accept, then ownership moves; renouncing afterwards clears it.
#[test]
fn ownership_two_step_transfer_and_renounce() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&new_owner, &1000);
    assert_eq!(client.get_owner(), Some(owner)); // pending, not yet accepted

    client.accept_ownership();
    assert_eq!(client.get_owner(), Some(new_owner.clone()));

    client.renounce_ownership();
    assert_eq!(client.get_owner(), None);
}

/// `accept_ownership` fails with no pending transfer, and a transfer whose
/// ledger deadline has lapsed can no longer be accepted. Both surface as
/// `NoPendingTransfer` (2200): the pending entry lives in temporary storage with
/// a TTL equal to its deadline, so past the deadline it is evicted rather than
/// merely flagged. The distinct `TransferExpired` (2203) path requires writing
/// the entry with a longer TTL than its deadline, only reachable via OZ's
/// internal storage key — it is covered by stellar-access's own tests.
#[test]
fn accept_ownership_rejects_missing_or_lapsed() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let new_owner = Address::generate(&env);

    assert_eq!(
        client.try_accept_ownership(),
        Err(Ok(soroban_sdk::Error::from_contract_error(2200)))
    );

    client.transfer_ownership(&new_owner, &100);
    env.ledger().set_sequence_number(101);
    assert_eq!(
        client.try_accept_ownership(),
        Err(Ok(soroban_sdk::Error::from_contract_error(2200)))
    );
}

/// Every privileged setter is gated by `#[only_owner]`: with the owner's
/// authorization withheld, each call is rejected.
#[test]
fn privileged_setters_require_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let peer = BytesN::from_array(&env, &[1u8; 32]);
    let hash = BytesN::from_array(&env, &[2u8; 32]);
    let addr = Address::generate(&env);

    env.mock_auths(&[]); // owner no longer authorizes anything
    assert!(client.try_set_peer(&3, &peer, &7, &u64::MAX).is_err());
    assert!(client.try_set_threshold(&1).is_err());
    assert!(client.try_set_transceiver(&addr).is_err());
    assert!(client.try_remove_transceiver(&addr).is_err());
    assert!(client.try_set_outbound_limit(&100).is_err());
    assert!(client.try_set_inbound_limit(&3, &100).is_err());
    assert!(client.try_upgrade(&hash).is_err());
}

/// `pause` is callable by the owner or a designated pauser; anyone else is
/// rejected with `NotAdminOrPauser`.
#[test]
fn pause_allows_owner_or_pauser_only() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let pauser = Address::generate(&env);
    client.transfer_pauser(&owner, &Some(pauser.clone()));
    assert_eq!(client.get_pauser(), Some(pauser.clone()));

    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_pause(&stranger),
        Err(Ok(soroban_sdk::Error::from_contract_error(13)))
    );

    client.pause(&pauser);
    assert!(client.paused());
}

/// `unpause` ignores its caller argument and requires the owner's auth, so the
/// pauser (who may pause) cannot unpause.
#[test]
fn unpause_requires_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let pauser = Address::generate(&env);
    client.transfer_pauser(&owner, &Some(pauser.clone()));
    client.pause(&pauser);
    assert!(client.paused());

    env.mock_auths(&[]); // withhold the owner's auth
    assert!(client.try_unpause(&pauser).is_err());

    env.mock_all_auths();
    client.unpause(&owner);
    assert!(!client.paused());
}

/// `transfer_pauser` is owner-or-pauser; `None` clears the role and an
/// unauthorized caller is rejected with `NotAdminOrPauser`.
#[test]
fn transfer_pauser_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let pauser = Address::generate(&env);

    client.transfer_pauser(&owner, &Some(pauser.clone()));
    assert_eq!(client.get_pauser(), Some(pauser.clone()));

    // The pauser can hand the role back.
    client.transfer_pauser(&pauser, &None);
    assert_eq!(client.get_pauser(), None);

    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_transfer_pauser(&stranger, &Some(stranger.clone())),
        Err(Ok(NttManagerError::NotAdminOrPauser))
    );
}
