#![cfg(test)]

mod common;
#[path = "common/token.rs"]
mod token;

use common::{setup_manager, setup_manager_with_token};
use soroban_ntt_client::Mode;
use soroban_sdk::Env;
use token::MockToken;

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

#[test]
fn constructor_queries_token_decimals() {
    // SAC tokens are fixed at 7 decimals; an 18-decimal token proves the
    // constructor reads decimals from the token rather than assuming 7.
    let env = Env::default();
    let token = env.register(MockToken, (18u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Locking, 1, 1000, 3600, &token);
    assert_eq!(client.token_decimals(), 18);
}
