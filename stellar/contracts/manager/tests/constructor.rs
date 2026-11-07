#![cfg(test)]

mod common;

use common::{setup_manager, setup_manager_with_token};
use soroban_ntt_client::Mode;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// The constructor persists every config field and initial counter — owner,
/// token, mode, chain id, threshold 0, version 1, full outbound capacity, and
/// the rate-limit duration from the argument (not the 86400 default).
#[test]
fn constructor_stores_config() {
    let env = Env::default();
    let (owner, token, client) = setup_manager(&env, Mode::Locking, 1, 1000, 3600);

    assert_eq!(client.get_owner(), Some(owner));
    assert_eq!(client.get_token(), token);
    assert_eq!(client.get_mode(), Mode::Locking);
    assert_eq!(client.get_chain_id(), 1);
    assert_eq!(client.token_decimals(), 7);
    assert_eq!(client.get_threshold(), 0);
    assert_eq!(client.get_transceiver_count(), 0);
    assert_eq!(client.get_enabled_bitmap(), 0);
    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_pauser(), None);
    assert!(!client.paused());
    // 3600 differs from the 86400 storage default, proving it is taken from the arg.
    assert_eq!(client.get_rate_limit_duration(), 3600);

    let params = client.get_outbound_limit_params();
    assert_eq!(params.limit, 1000);
    assert_eq!(params.current_capacity, 1000);
}

/// A zero chain id is never a valid Wormhole identity; the constructor rejects it
/// up front rather than persisting a manager whose own chain can never match an
/// inbound message's `to_chain`.
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn constructor_rejects_zero_chain_id() {
    let env = Env::default();
    setup_manager(&env, Mode::Locking, 0, 1000, 3600);
}

/// The constructor reads decimals from the token contract rather than assuming
/// the address is a token, so a deploy over any other address fails.
#[test]
#[should_panic(expected = "Error(Storage, MissingValue)")]
fn constructor_rejects_non_token() {
    let env = Env::default();
    let not_a_token = Address::generate(&env);
    setup_manager_with_token(&env, Mode::Locking, 1, 1000, 3600, &not_a_token);
}
