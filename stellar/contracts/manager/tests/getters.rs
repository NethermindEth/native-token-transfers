#![cfg(test)]

use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, BytesN, Env};
use stellar_ntt_manager::{state::Mode, ManagerContract, ManagerContractClient};

#[contract]
struct DummyContract;

#[contractimpl]
impl DummyContract {}

#[test]
fn test_get_outbound_limit_params_defaults_unlimited() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let params = env.as_contract(&contract_id, || {
        ManagerContract::get_outbound_limit_params(env.clone())
    });
    assert_eq!(params.limit, u64::MAX);
    assert_eq!(params.current_capacity, u64::MAX);
}

#[test]
fn test_get_outbound_capacity_defaults_unlimited() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let capacity = env.as_contract(&contract_id, || {
        ManagerContract::get_outbound_capacity(env.clone())
    });
    assert_eq!(capacity, u64::MAX);
}

#[test]
fn test_get_transceiver_info_none_when_missing() {
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

    // Verify that get_transceiver_info returns None for a missing transceiver
    assert!(client.get_transceiver_info(&0).is_none());
    assert!(client.get_transceiver_info(&1).is_none());
}

#[test]
fn test_get_peer_none_when_missing() {
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

    // Verify that get_peer returns None for an unknown chain ID
    assert!(client.get_peer(&2).is_none());
    assert!(client.get_peer(&100).is_none());
}

#[test]
fn test_get_inbound_limit_params_none_when_no_peer() {
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

    // Verify that get_inbound_limit_params returns None if peer not registered
    assert!(client.get_inbound_limit_params(&2).is_none());
    assert!(client.get_inbound_limit_params(&100).is_none());
}

#[test]
fn test_get_next_sequence_defaults_to_1() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let sequence = env.as_contract(&contract_id, || {
        ManagerContract::get_next_sequence(env.clone())
    });
    assert_eq!(sequence, 1);
}

#[test]
fn test_is_message_executed_false_when_no_attestation() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let digest = BytesN::from_array(&env, &[0u8; 32]);
    let executed = env.as_contract(&contract_id, || {
        ManagerContract::is_message_executed(env.clone(), digest)
    });
    assert!(!executed);
}

#[test]
fn test_get_attestation_info_none_when_missing() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let digest = BytesN::from_array(&env, &[0u8; 32]);
    let info = env.as_contract(&contract_id, || {
        ManagerContract::get_attestation_info(env.clone(), digest)
    });
    assert!(info.is_none());
}

#[test]
fn test_get_outbound_queue_item_none_when_missing() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let info = env.as_contract(&contract_id, || {
        ManagerContract::get_outbound_queue_item(env.clone(), 1u64)
    });
    assert!(info.is_none());
}

#[test]
fn test_get_inbound_queue_item_none_when_missing() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());
    let digest = BytesN::from_array(&env, &[0u8; 32]);
    let info = env.as_contract(&contract_id, || {
        ManagerContract::get_inbound_queue_item(env.clone(), digest)
    });
    assert!(info.is_none());
}
