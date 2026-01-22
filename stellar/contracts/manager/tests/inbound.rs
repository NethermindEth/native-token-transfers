#![cfg(test)]

use soroban_sdk::{
    address_payload::AddressPayload, contract, contractimpl, testutils::Address as _, Address,
    Bytes, BytesN, Env,
};
use stellar_ntt_manager::{
    errors::NttManagerError,
    messages::{NativeTokenTransfer, NttManagerMessage, TrimmedAmount},
    state::Mode,
    ManagerContract, ManagerContractClient,
};

fn generate_account_address(env: &Env) -> Address {
    let addr = Address::generate(env);
    let bytes = match addr.to_payload().unwrap() {
        AddressPayload::AccountIdPublicKeyEd25519(b) => b,
        AddressPayload::ContractIdHash(b) => b,
    };
    Address::from_payload(env, AddressPayload::AccountIdPublicKeyEd25519(bytes))
}

#[contract]
struct Token;

#[contractimpl]
impl Token {
    pub fn mint(_env: Env, _to: Address, _amount: i128) {}
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 {
        7
    }
}

fn setup_test(env: &Env) -> (Address, Address, Address, ManagerContractClient<'_>) {
    let admin = Address::generate(env);

    let token = env.register(Token, ());
    let mode = Mode::Burning;
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
    let client = ManagerContractClient::new(env, &contract_id);

    env.mock_all_auths();

    (admin, token, contract_id, client)
}

fn create_test_message(
    env: &Env,
    amount: u64,
    recipient: &Address,
    _contract_id: &Address,
) -> (NttManagerMessage, Bytes) {
    let payload = NativeTokenTransfer {
        amount: TrimmedAmount {
            amount,
            decimals: 7,
        },
        source_token: BytesN::from_array(env, &[3u8; 32]),
        to: match recipient.to_payload().unwrap() {
            soroban_sdk::address_payload::AddressPayload::AccountIdPublicKeyEd25519(bytes) => bytes,
            soroban_sdk::address_payload::AddressPayload::ContractIdHash(bytes) => bytes,
        },
        to_chain: 1,
        additional_payload: None,
    };
    let message = NttManagerMessage {
        id: BytesN::from_array(env, &[1u8; 32]),
        sender: BytesN::from_array(env, &[2u8; 32]),
        payload,
    };
    let bytes = message.to_bytes(env).unwrap();
    (message, bytes)
}

#[test]
fn test_attestation_received_requires_auth() {
    let env = Env::default();
    let (_, _, _, client) = setup_test(&env);
    let transceiver = Address::generate(&env);

    let result = client.try_attestation_received(
        &transceiver,
        &2u32,
        &BytesN::from_array(&env, &[1u8; 32]),
        &Bytes::new(&env),
    );

    assert!(result.is_err());
}

#[test]
fn test_attestation_received_requires_not_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, _, client) = setup_test(&env);
    let transceiver = Address::generate(&env);

    client.pause(&admin);

    let result = client.try_attestation_received(
        &transceiver,
        &2u32,
        &BytesN::from_array(&env, &[1u8; 32]),
        &Bytes::new(&env),
    );

    match result {
        Err(Ok(NttManagerError::ContractPaused)) => (),
        _ => panic!("Expected ContractPaused, got {:?}", result),
    }
}

#[test]
fn test_attestation_received_rejects_if_transceiver_not_enabled() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, _, client) = setup_test(&env);
    let transceiver = Address::generate(&env);

    client.set_transceiver(&admin, &transceiver);

    let t2 = Address::generate(&env);
    client.set_transceiver(&admin, &t2);

    client.remove_transceiver(&admin, &transceiver);

    let result = client.try_attestation_received(
        &transceiver,
        &2u32,
        &BytesN::from_array(&env, &[1u8; 32]),
        &Bytes::new(&env),
    );

    match result {
        Err(Ok(NttManagerError::TransceiverNotRegistered))
        | Err(Ok(NttManagerError::TransceiverNotEnabled)) => (),
        _ => panic!(
            "Expected TransceiverNotRegistered or TransceiverNotEnabled, got {:?}",
            result
        ),
    }
}

#[test]
fn test_attestation_received_updates_attestation_info() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, contract_id, client) = setup_test(&env);
    let transceiver = Address::generate(&env);
    let source_chain = 2u32;
    let source_manager = BytesN::from_array(&env, &[4u8; 32]);
    let recipient = generate_account_address(&env);
    let (message, payload) = create_test_message(&env, 100, &recipient, &contract_id);
    let digest = message.compute_digest(&env, source_chain as u16).unwrap();

    client.set_transceiver(&admin, &transceiver);
    client.set_peer(&admin, &source_chain, &source_manager, &7u32, &1000u64);
    client.set_threshold(&admin, &1);

    client.attestation_received(&transceiver, &source_chain, &source_manager, &payload);

    let info = client.get_attestation_info(&digest).unwrap();
    assert_eq!(info.attested_transceivers, 1 << 0);
    assert!(info.executed);
}

#[test]
fn test_attestation_received_sets_approved_when_threshold_met() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, contract_id, client) = setup_test(&env);
    let t1 = Address::generate(&env);
    let t2 = Address::generate(&env);
    let source_chain = 2u32;
    let source_manager = BytesN::from_array(&env, &[4u8; 32]);
    let (_message, payload) =
        create_test_message(&env, 100, &generate_account_address(&env), &contract_id);

    client.set_transceiver(&admin, &t1);
    client.set_transceiver(&admin, &t2);
    client.set_peer(&admin, &source_chain, &source_manager, &7u32, &1000u64);
    client.set_threshold(&admin, &2);

    // First attestation
    let res1 = client.attestation_received(&t1, &source_chain, &source_manager, &payload);
    assert!(!res1.approved);
    assert!(!res1.executed);

    // Second attestation
    let res2 = client.attestation_received(&t2, &source_chain, &source_manager, &payload);
    assert!(res2.approved);
    assert!(res2.executed);
}

#[test]
fn test_attestation_received_does_not_double_count_same_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, contract_id, client) = setup_test(&env);
    let t1 = Address::generate(&env);
    let source_chain = 2u32;
    let source_manager = BytesN::from_array(&env, &[4u8; 32]);
    let (_message, payload) =
        create_test_message(&env, 100, &generate_account_address(&env), &contract_id);

    client.set_transceiver(&admin, &t1);

    let t3 = Address::generate(&env);
    client.set_transceiver(&admin, &t3);

    client.set_peer(&admin, &source_chain, &source_manager, &7u32, &1000u64);
    client.set_threshold(&admin, &2);

    client.attestation_received(&t1, &source_chain, &source_manager, &payload);

    let result = client.try_attestation_received(&t1, &source_chain, &source_manager, &payload);
    match result {
        Err(Ok(NttManagerError::TransceiverAlreadyAttested)) => (),
        _ => panic!("Expected TransceiverAlreadyAttested, got {:?}", result),
    }
}

#[test]
fn test_attestation_received_marks_executed_when_release_happens() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, contract_id, client) = setup_test(&env);
    let t1 = Address::generate(&env);
    let source_chain = 2u32;
    let source_manager = BytesN::from_array(&env, &[4u8; 32]);
    let recipient = generate_account_address(&env);
    let (_message, payload) = create_test_message(&env, 100, &recipient, &contract_id);
    let digest = _message.compute_digest(&env, source_chain as u16).unwrap();

    client.set_transceiver(&admin, &t1);
    client.set_peer(&admin, &source_chain, &source_manager, &7u32, &1000u64);
    client.set_threshold(&admin, &1);

    let res = client.attestation_received(&t1, &source_chain, &source_manager, &payload);
    assert!(res.executed);

    let info = client.get_attestation_info(&digest).unwrap();
    assert!(info.executed);
}

#[test]
fn test_attestation_received_queues_when_inbound_rate_limited() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, contract_id, client) = setup_test(&env);
    let t1 = Address::generate(&env);
    let source_chain = 2u32;
    let source_manager = BytesN::from_array(&env, &[4u8; 32]);
    let recipient = generate_account_address(&env);


    let (message, payload) = create_test_message(&env, 100, &recipient, &contract_id);
    let digest = message.compute_digest(&env, source_chain as u16).unwrap();

    client.set_transceiver(&admin, &t1);
    // Set low rate limit
    client.set_peer(&admin, &source_chain, &source_manager, &7u32, &50u64);
    client.set_threshold(&admin, &1);

    let res = client.attestation_received(&t1, &source_chain, &source_manager, &payload);
    assert!(res.approved);
    assert!(!res.executed);
    assert!(res.queued);

    // Verify it's in the queue
    let queue_item = client.get_inbound_queue_item(&digest).unwrap();
    // Compare string representations
    let recipient_str = recipient.to_string();
    let queue_recipient_str = queue_item.recipient.to_string();
    assert_eq!(queue_recipient_str, recipient_str);
    assert_eq!(queue_item.amount, 100);
}
