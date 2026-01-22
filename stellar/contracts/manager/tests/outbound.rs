#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use stellar_ntt_manager::{
    errors::NttManagerError,
    messages::TrimmedAmount,
    state::{DataKey, Mode, OutboundQueuedTransfer},
    ManagerContract, ManagerContractClient,
};

#[test]
fn test_complete_queued_transfer_requires_not_paused() {
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

    client.pause(&admin);
    assert!(client.is_paused());

    let result = client.try_complete_queued_transfer(&1u64);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::ContractPaused),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::ContractPaused)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_complete_queued_transfer_requires_not_reentering() {
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

    env.as_contract(&contract_id, || {
        env.storage().temporary().set(&DataKey::Reentering, &true);
    });

    let result = client.try_complete_queued_transfer(&1u64);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::Reentering),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::Reentering)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_complete_queued_transfer_errors_when_not_queued() {
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

    let result = client.try_complete_queued_transfer(&12345u64);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::TransferNotQueued),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::TransferNotQueued)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_complete_queued_transfer_errors_when_not_releasable() {
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

    let sequence = 1u64;
    let release_timestamp = env.ledger().timestamp() + 3600;

    env.as_contract(&contract_id, || {
        let queued = OutboundQueuedTransfer {
            sender: Address::generate(&env),
            amount: TrimmedAmount {
                amount: 100,
                decimals: 7,
            },
            recipient_chain: 2,
            recipient_ntt_manager: BytesN::from_array(&env, &[1u8; 32]),
            recipient: BytesN::from_array(&env, &[2u8; 32]),
            source_token: BytesN::from_array(&env, &[3u8; 32]),
            release_timestamp,
            additional_payload: None,
        };
        env.storage()
            .persistent()
            .set(&DataKey::OutboundQueue(sequence), &queued);
    });

    let result = client.try_complete_queued_transfer(&sequence);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::TransferNotReleasable),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::TransferNotReleasable)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_complete_queued_transfer_errors_when_still_rate_limited() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let mode = Mode::Locking;
    let chain_id = 1u32;
    let outbound_limit = 100u64;
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

    let sequence = 1u64;
    let release_timestamp = env.ledger().timestamp();

    env.as_contract(&contract_id, || {
        let queued = OutboundQueuedTransfer {
            sender: Address::generate(&env),
            amount: TrimmedAmount {
                amount: 150, // > outbound_limit
                decimals: 7,
            },
            recipient_chain: 2,
            recipient_ntt_manager: BytesN::from_array(&env, &[1u8; 32]),
            recipient: BytesN::from_array(&env, &[2u8; 32]),
            source_token: BytesN::from_array(&env, &[3u8; 32]),
            release_timestamp,
            additional_payload: None,
        };
        env.storage()
            .persistent()
            .set(&DataKey::OutboundQueue(sequence), &queued);
    });

    let result = client.try_complete_queued_transfer(&sequence);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::TransferExceedsRateLimit),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::TransferExceedsRateLimit)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_cancel_queued_transfer_requires_sender_auth() {
    let env = Env::default();

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

    let sender = Address::generate(&env);
    let sequence = 1u64;

    let result = client.try_cancel_queued_transfer(&sender, &sequence);
    assert!(result.is_err());

    if let Err(Ok(err)) = result {
        panic!("Expected auth error, got NttManagerError::{:?}", err);
    }

    env.mock_all_auths();
    let result = client.try_cancel_queued_transfer(&sender, &sequence);
    match result {
        Err(Ok(NttManagerError::TransferNotQueued)) => {}
        _ => panic!(
            "Expected Err(Ok(NttManagerError::TransferNotQueued)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_cancel_queued_transfer_errors_when_not_queued() {
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

    let sender = Address::generate(&env);
    let sequence = 1u64;

    let result = client.try_cancel_queued_transfer(&sender, &sequence);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::TransferNotQueued),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::TransferNotQueued)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_cancel_queued_transfer_errors_when_canceller_not_sender() {
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

    let sender = Address::generate(&env);
    let canceller = Address::generate(&env);
    let sequence = 1u64;

    env.as_contract(&contract_id, || {
        let queued = OutboundQueuedTransfer {
            sender: sender.clone(),
            amount: TrimmedAmount {
                amount: 100,
                decimals: 7,
            },
            recipient_chain: 2,
            recipient_ntt_manager: BytesN::from_array(&env, &[1u8; 32]),
            recipient: BytesN::from_array(&env, &[2u8; 32]),
            source_token: BytesN::from_array(&env, &[3u8; 32]),
            release_timestamp: 0,
            additional_payload: None,
        };
        env.storage()
            .persistent()
            .set(&DataKey::OutboundQueue(sequence), &queued);
    });

    let result = client.try_cancel_queued_transfer(&canceller, &sequence);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::CancellerNotSender),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::CancellerNotSender)), got {:?}",
            result
        ),
    }
}
