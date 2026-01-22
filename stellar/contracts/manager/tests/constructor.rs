#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use stellar_ntt_manager::{state::Mode, ManagerContract, ManagerContractClient};

#[test]
fn test_constructor_stores_admin_token_mode_chain_id() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let mode = Mode::Locking;
    let chain_id = 1u32;
    let outbound_limit = 1000u64;
    let rate_limit_duration = 3600u64;

    let contract_id = env.register(
        ManagerContract,
        (
            &admin,
            &token,
            &mode,
            &chain_id,
            &outbound_limit,
            &rate_limit_duration,
        ),
    );
    let client = ManagerContractClient::new(&env, &contract_id);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_token(), token);
    assert_eq!(client.get_mode(), mode);
    assert_eq!(client.get_chain_id(), chain_id);
    assert_eq!(client.token_decimals(), 7); // Stellar asset contracts have 7 decimals by default
    assert_eq!(client.get_threshold(), 0);
    assert_eq!(client.get_transceiver_count(), 0);
    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_rate_limit_duration(), rate_limit_duration);
    assert_eq!(client.is_paused(), false);

    let rate_limit_params = client.get_outbound_limit_params();
    assert_eq!(rate_limit_params.limit, outbound_limit);
    assert_eq!(rate_limit_params.current_capacity, outbound_limit);
}

#[test]
fn test_constructor_initializes_version_default_1() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let mode = Mode::Locking;
    let chain_id = 1u32;
    let outbound_limit = 1000u64;
    let rate_limit_duration = 3600u64;

    let contract_id = env.register(
        ManagerContract,
        (
            &admin,
            &token,
            &mode,
            &chain_id,
            &outbound_limit,
            &rate_limit_duration,
        ),
    );
    let client = ManagerContractClient::new(&env, &contract_id);

    assert_eq!(client.get_version(), 1);
}

#[test]
fn test_constructor_sets_rate_limit_duration_or_default() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let mode = Mode::Locking;
    let chain_id = 1u32;
    let outbound_limit = 1000u64;
    let rate_limit_duration = 7200u64;

    let contract_id = env.register(
        ManagerContract,
        (
            &admin,
            &token,
            &mode,
            &chain_id,
            &outbound_limit,
            &rate_limit_duration,
        ),
    );
    let client = ManagerContractClient::new(&env, &contract_id);

    assert_eq!(client.get_rate_limit_duration(), rate_limit_duration);
}
