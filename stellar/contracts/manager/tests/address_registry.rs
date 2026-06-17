#![cfg(test)]

mod common;

use common::setup_manager;
use soroban_ntt_client::{Mode, NttManagerError};
use soroban_sdk::{
    address_payload::AddressPayload, testutils::Address as _, Address, BytesN, Env,
};
use wormhole_soroban_client::hash_address;

const OUR_CHAIN: u32 = 2;

/// An account and a contract built from identical raw bytes must hash to
/// distinct keys — each the canonical `hash_address` an off-chain caller
/// computes — and resolve back to its own kind. This is the discriminator-loss
/// bug the registry fixes.
#[test]
fn same_bytes_account_and_contract_resolve_to_distinct_kinds() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    let raw = BytesN::from_array(&env, &[7u8; 32]);
    let account = Address::from_payload(&env, AddressPayload::AccountIdPublicKeyEd25519(raw.clone()));
    let contract = Address::from_payload(&env, AddressPayload::ContractIdHash(raw));

    let account_hash = client.record_address(&account);
    let contract_hash = client.record_address(&contract);

    assert_eq!(account_hash, hash_address(&env, &account));
    assert_eq!(contract_hash, hash_address(&env, &contract));
    assert_ne!(account_hash, contract_hash);
    assert_eq!(client.get_address_from_hash(&account_hash), account);
    assert_eq!(client.get_address_from_hash(&contract_hash), contract);
}

#[test]
fn resolve_unknown_hash_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    let unknown = BytesN::from_array(&env, &[9u8; 32]);
    assert_eq!(
        client.try_get_address_from_hash(&unknown),
        Err(Ok(NttManagerError::RecipientNotRegistered))
    );
}

#[test]
fn record_address_is_idempotent_and_permissionless() {
    let env = Env::default();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    let contract = Address::generate(&env);
    let first = client.record_address(&contract);
    let second = client.record_address(&contract);

    assert_eq!(first, second);
    // No auth was mocked, yet the call succeeds and demands no signature.
    assert!(env.auths().is_empty());
}
