#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec};
use wormhole_interface::{ConsistencyLevel, Error as WormholeError};

#[contract]
pub struct TransceiverContract;

#[contractimpl]
impl TransceiverContract {
    pub fn init(env: Env, manager: Address, wormhole_core: Address) {
        set_manager_internal(&env, &manager);
        set_wormhole_core_internal(&env, &wormhole_core);
    }

    pub fn set_manager(env: Env, manager: Address) {
        set_manager_internal(&env, &manager);
    }

    pub fn get_manager(env: Env) -> Address {
        get_manager_internal(&env).unwrap_or_else(|| panic!("manager not set"))
    }

    pub fn set_wormhole_core(env: Env, wormhole_core: Address) {
        set_wormhole_core_internal(&env, &wormhole_core);
    }

    pub fn get_wormhole_core(env: Env) -> Address {
        get_wormhole_core_internal(&env).unwrap_or_else(|| panic!("wormhole core not set"))
    }

    pub fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>) {
        set_peer_internal(&env, chain_id, &emitter);
    }

    pub fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>> {
        get_peer_internal(&env, chain_id)
    }

    /// Send an NTT message. For now:
    /// - Uses the stored Wormhole core address
    /// - Uses this contract's address as the emitter
    /// - Uses a fixed nonce = 0 and ConsistencyLevel::Confirmed
    /// - Returns the sequence number from Wormhole core
    pub fn send(env: Env, _dst_chain_id: u32, payload: Bytes) -> u64 {
        let core_addr =
            get_wormhole_core_internal(&env).unwrap_or_else(|| panic!("wormhole core not set"));

        let emitter = env.current_contract_address();
        let nonce: u32 = 0;
        let consistency = ConsistencyLevel::Confirmed;

        // Build args Vec<Val> manually
        let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
        args.push_back((&emitter).into_val(&env));
        args.push_back((&nonce).into_val(&env));
        args.push_back((&payload).into_val(&env));
        args.push_back((&consistency).into_val(&env));

        let res: Result<u64, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "post_message"), args);

        match res {
            Ok(seq) => seq,
            Err(_e) => panic!("post_message failed"),
        }
    }

    /// Receive a Wormhole VAA and forward it to the NTT manager.
    ///
    /// TODO: implement VAA verification + forwarding.
    pub fn receive_vaa(env: Env, vaa: Bytes) {
        let _ = (env, vaa);
        panic!("receive_vaa is not implemented");
    }
}

// Helpers
// TODO: it should be probably in the contract interface crate

const KEY_MANAGER: &str = "m";
const KEY_WORMHOLE_CORE: &str = "w";
const KEY_PEER: &str = "p";

fn manager_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_MANAGER)
}

fn wormhole_core_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_WORMHOLE_CORE)
}

fn peer_key(env: &Env, chain_id: u32) -> (Symbol, u32) {
    (Symbol::new(env, KEY_PEER), chain_id)
}

fn set_manager_internal(env: &Env, manager: &Address) {
    let key = manager_key(env);
    env.storage().instance().set(&key, manager);
}

fn get_manager_internal(env: &Env) -> Option<Address> {
    let key = manager_key(env);
    env.storage().instance().get(&key)
}

fn set_wormhole_core_internal(env: &Env, wormhole_core: &Address) {
    let key = wormhole_core_key(env);
    env.storage().instance().set(&key, wormhole_core);
}

fn get_wormhole_core_internal(env: &Env) -> Option<Address> {
    let key = wormhole_core_key(env);
    env.storage().instance().get(&key)
}

fn set_peer_internal(env: &Env, chain_id: u32, emitter: &BytesN<32>) {
    let key = peer_key(env, chain_id);
    env.storage().instance().set(&key, emitter);
}

fn get_peer_internal(env: &Env, chain_id: u32) -> Option<BytesN<32>> {
    let key = peer_key(env, chain_id);
    env.storage().instance().get(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl, testutils::Address as _, Address, Bytes, BytesN, Env,
    };
    use wormhole_interface::{ConsistencyLevel, Error as WormholeError};

    fn setup() -> (Env, TransceiverContractClient<'static>, Address, Address) {
        let env = Env::default();

        let contract_id = env.register(TransceiverContract, ());
        let client = TransceiverContractClient::new(&env, &contract_id);

        let manager = Address::generate(&env);
        let wormhole_core = Address::generate(&env);

        (env, client, manager, wormhole_core)
    }

    #[test]
    fn init_sets_manager_and_core() {
        let (_env, client, manager, wormhole_core) = setup();

        client.init(&manager, &wormhole_core);

        let got_manager = client.get_manager();
        let got_core = client.get_wormhole_core();

        assert_eq!(got_manager, manager);
        assert_eq!(got_core, wormhole_core);
    }

    #[test]
    fn set_manager_overrides_previous_value() {
        let (env, client, manager, wormhole_core) = setup();

        client.init(&manager, &wormhole_core);

        let new_manager = Address::generate(&env);
        client.set_manager(&new_manager);

        let got_manager = client.get_manager();
        assert_eq!(got_manager, new_manager);
    }

    #[test]
    fn set_peer_and_get_peer_roundtrip() {
        let (env, client, manager, wormhole_core) = setup();

        client.init(&manager, &wormhole_core);

        let chain_id: u32 = 2;
        let emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);

        client.set_peer(&chain_id, &emitter);

        let got = client.get_peer(&chain_id).expect("peer should be set");

        assert_eq!(got, emitter);
    }

    // TODO E2E tests?
    #[contract]
    pub struct DummyWormholeCore;

    #[contractimpl]
    impl DummyWormholeCore {
        pub fn post_message(
            _env: Env,
            _emitter: Address,
            _nonce: u32,
            _payload: Bytes,
            _consistency_level: ConsistencyLevel,
        ) -> Result<u64, WormholeError> {
            Ok(42)
        }
    }

    #[test]
    fn send_calls_core_and_returns_sequence() {
        let env = Env::default();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let core_id = env.register(DummyWormholeCore, ());

        let manager = Address::generate(&env);
        transceiver_client.init(&manager, &core_id);

        let dst_chain_id: u32 = 2;
        let payload = Bytes::from_array(&env, &[1u8, 2, 3]);

        let seq = transceiver_client.send(&dst_chain_id, &payload);
        assert_eq!(seq, 42);
    }

    #[test]
    #[should_panic(expected = "wormhole core not set")]
    fn send_panics_if_core_not_set() {
        let env = Env::default();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let dst_chain_id: u32 = 2;
        let payload = Bytes::from_array(&env, &[1u8, 2, 3]);

        let _ = transceiver_client.send(&dst_chain_id, &payload);
    }
}
