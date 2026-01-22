#![cfg(test)]

use soroban_sdk::{contract, contractimpl, BytesN, Env};
use stellar_ntt_manager::errors::NttManagerError;
use stellar_ntt_manager::state::{
    is_paused, require_not_paused, require_not_reentering, sequence_to_message_id, set_reentering,
    use_message_sequence, DataKey,
};

#[contract]
struct DummyContract;

#[contractimpl]
impl DummyContract {}

#[test]
fn test_is_paused_default_false() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        assert!(!is_paused(&env));
    });
}

#[test]
fn test_require_not_paused_ok_when_false() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        assert!(require_not_paused(&env).is_ok());
    });
}

#[test]
fn test_require_not_paused_errors_when_true() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::Paused, &true);
        match require_not_paused(&env) {
            Err(NttManagerError::ContractPaused) => (),
            _ => panic!("Expected ContractPaused error"),
        }
    });
}

#[test]
fn test_require_not_reentering_ok_when_false() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        assert!(require_not_reentering(&env).is_ok());
    });
}

#[test]
fn test_require_not_reentering_errors_when_true() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        env.storage().temporary().set(&DataKey::Reentering, &true);
        match require_not_reentering(&env) {
            Err(NttManagerError::Reentering) => (),
            _ => panic!("Expected Reentering error"),
        }
    });
}

#[test]
fn test_set_reentering_sets_flag() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        set_reentering(&env, true);
        match require_not_reentering(&env) {
            Err(NttManagerError::Reentering) => (),
            _ => panic!("Expected Reentering error after set_reentering(true)"),
        }

        set_reentering(&env, false);
        assert!(require_not_reentering(&env).is_ok());
    });
}

#[test]
fn test_use_message_sequence_starts_at_1() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        assert_eq!(use_message_sequence(&env), 1);
    });
}

#[test]
fn test_use_message_sequence_increments_monotonically() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        assert_eq!(use_message_sequence(&env), 1);
        assert_eq!(use_message_sequence(&env), 2);
        assert_eq!(use_message_sequence(&env), 3);
        assert_eq!(use_message_sequence(&env), 4);
    });
}

#[test]
fn test_sequence_to_message_id_encodes_big_endian_u64() {
    let env = Env::default();
    let contract_id = env.register(DummyContract, ());

    env.as_contract(&contract_id, || {
        let sequence = 0x1234567890ABCDEFu64;
        let message_id = sequence_to_message_id(&env, sequence);

        let mut expected = [0u8; 32];
        expected[24..32].copy_from_slice(&sequence.to_be_bytes());

        assert_eq!(message_id, BytesN::from_array(&env, &expected));
    });
}
