#![cfg(test)]

use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, BytesN, Env};
use stellar_ntt_manager::{
    errors::NttManagerError,
    peers::NttManagerPeer,
    rate_limit::RateLimitParams,
    state::{DataKey, Mode},
    ManagerContract, ManagerContractClient,
};

#[contract]
struct DummyContract;

#[contractimpl]
impl DummyContract {}

#[test]
fn test_quote_transfer_errors_when_peer_missing() {
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

    let recipient_chain = 2u32;
    let amount = 100i128;

    let result = client.try_quote_transfer(&amount, &recipient_chain);

    match result {
        Err(Ok(err)) => assert_eq!(err, NttManagerError::PeerNotFound),
        _ => panic!(
            "Expected Err(Ok(NttManagerError::PeerNotFound)), got {:?}",
            result
        ),
    }
}

#[test]
fn test_quote_transfer_errors_when_token_decimals_missing() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    let recipient_chain = 2u32;
    let amount = 100i128;

    let result = env.as_contract(&contract_id, || {
        // Setup peer in storage manually
        let peer = NttManagerPeer {
            address: BytesN::from_array(&env, &[1u8; 32]),
            token_decimals: 7,
            inbound_rate_limit: RateLimitParams::new(u64::MAX, &env),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Peer(recipient_chain), &peer);

        ManagerContract::quote_transfer(env.clone(), amount, recipient_chain)
    });

    match result {
        Err(NttManagerError::NotInitialized) => {}
        _ => panic!(
            "Expected Err(NttManagerError::NotInitialized), got {:?}",
            result
        ),
    }
}

#[test]
fn test_quote_transfer_returns_trimmed_and_dust() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    let recipient_chain = 2u32;
    let our_decimals = 7u32;
    let amount = 1005i128; // 1005 units with 7 decimals

    // peer has fewer decimals (7 -> 6)
    // target = min(8, 7, 6) = 6.
    // trimmed = 1005 / 10^(7-6) = 100.
    // dust = 1005 % 10 = 5.
    let (trimmed, dust) = env.as_contract(&contract_id, || {
        // Setup decimals in instance storage
        env.storage()
            .instance()
            .set(&DataKey::TokenDecimals, &our_decimals);

        let peer_decimals = 6u32;
        let peer = NttManagerPeer {
            address: BytesN::from_array(&env, &[1u8; 32]),
            token_decimals: peer_decimals,
            inbound_rate_limit: RateLimitParams::new(u64::MAX, &env),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Peer(recipient_chain), &peer);

        ManagerContract::quote_transfer(env.clone(), amount, recipient_chain).unwrap()
    });
    assert_eq!(trimmed, 100);
    assert_eq!(dust, 5);

    // Same decimals (7 -> 7)
    // target = min(8, 7, 7) = 7.
    // trimmed = 1005, dust = 0.
    let (trimmed, dust) = env.as_contract(&contract_id, || {
        let peer_decimals = 7u32;
        let peer = NttManagerPeer {
            address: BytesN::from_array(&env, &[1u8; 32]),
            token_decimals: peer_decimals,
            inbound_rate_limit: RateLimitParams::new(u64::MAX, &env),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Peer(recipient_chain + 1), &peer);

        ManagerContract::quote_transfer(env.clone(), amount, recipient_chain + 1).unwrap()
    });
    assert_eq!(trimmed, 1005);
    assert_eq!(dust, 0);

    // Peer has more decimals (7 -> 9)
    // target = min(8, 7, 9) = 7.
    // trimmed = 1005, dust = 0.
    let (trimmed, dust) = env.as_contract(&contract_id, || {
        let peer_decimals = 9u32;
        let peer = NttManagerPeer {
            address: BytesN::from_array(&env, &[1u8; 32]),
            token_decimals: peer_decimals,
            inbound_rate_limit: RateLimitParams::new(u64::MAX, &env),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Peer(recipient_chain + 2), &peer);

        ManagerContract::quote_transfer(env.clone(), amount, recipient_chain + 2).unwrap()
    });
    assert_eq!(trimmed, 1005);
    assert_eq!(dust, 0);
}
