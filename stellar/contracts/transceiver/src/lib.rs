#![no_std]

mod inbound;
mod outbound;
mod peers;
mod state;
mod storage;

use soroban_ntt_client::{
    address_to_bytes32, PeerInfo, TransceiverError, TransceiverInterface,
    WormholeTransceiverInterface,
};
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};

use state::TRANSCEIVER_TYPE;
use storage::InstanceStorage;

#[contract]
pub struct TransceiverContract;

#[contractimpl]
impl TransceiverContract {
    pub fn __constructor(
        env: Env,
        admin: Address,
        manager: Address,
        manager_id: BytesN<32>,
        wormhole_core: Address,
    ) -> Result<(), TransceiverError> {
        initialize(&env, &admin, &manager, &manager_id, &wormhole_core)
    }

    pub fn init(
        env: Env,
        admin: Address,
        manager: Address,
        manager_id: BytesN<32>,
        wormhole_core: Address,
    ) -> Result<(), TransceiverError> {
        admin.require_auth();
        initialize(&env, &admin, &manager, &manager_id, &wormhole_core)
    }

    pub fn is_initialized(env: Env) -> bool {
        InstanceStorage::new(&env).is_initialized()
    }
}

fn initialize(
    env: &Env,
    admin: &Address,
    manager: &Address,
    manager_id: &BytesN<32>,
    wormhole_core: &Address,
) -> Result<(), TransceiverError> {
    if address_to_bytes32(manager) != *manager_id {
        return Err(TransceiverError::InvalidManagerId);
    }
    InstanceStorage::new(env).initialize(admin, manager, manager_id, wormhole_core)
}

#[contractimpl]
impl TransceiverInterface for TransceiverContract {
    fn get_manager(env: Env) -> Result<Address, TransceiverError> {
        InstanceStorage::new(&env).manager()
    }

    fn get_manager_id(env: Env) -> Result<BytesN<32>, TransceiverError> {
        InstanceStorage::new(&env).manager_id()
    }

    fn get_transceiver_type(env: Env) -> Bytes {
        Bytes::from_array(&env, &TRANSCEIVER_TYPE)
    }

    fn send_message(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    ) -> Result<(), TransceiverError> {
        outbound::send_message(&env, recipient_chain, recipient_manager, manager_payload)
    }

    fn get_admin(env: Env) -> Result<Address, TransceiverError> {
        InstanceStorage::new(&env).admin()
    }

    fn set_admin(env: Env, new_admin: Address) -> Result<(), TransceiverError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin_auth()?;
        new_admin.require_auth();
        storage.set_admin(&new_admin);
        Ok(())
    }

    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), TransceiverError> {
        InstanceStorage::new(&env).require_admin_auth()?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }
}

#[contractimpl]
impl WormholeTransceiverInterface for TransceiverContract {
    fn get_wormhole_core(env: Env) -> Result<Address, TransceiverError> {
        InstanceStorage::new(&env).wormhole_core()
    }

    fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>) -> Result<(), TransceiverError> {
        peers::set_peer(&env, chain_id, emitter)
    }

    fn update_peer(env: Env, chain_id: u32, emitter: BytesN<32>) -> Result<(), TransceiverError> {
        peers::update_peer(&env, chain_id, emitter)
    }

    fn set_peer_enabled(
        env: Env,
        chain_id: u32,
        enabled: bool,
    ) -> Result<(), TransceiverError> {
        peers::set_peer_enabled(&env, chain_id, enabled)
    }

    fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>> {
        peers::get_peer(&env, chain_id)
    }

    fn get_peer_info(env: Env, chain_id: u32) -> Option<PeerInfo> {
        peers::get_peer_info(&env, chain_id)
    }

    fn is_peer_enabled(env: Env, chain_id: u32) -> bool {
        peers::is_peer_enabled(&env, chain_id)
    }

    fn receive_message(env: Env, vaa_bytes: Bytes) -> Result<(), TransceiverError> {
        inbound::receive_message(&env, vaa_bytes)
    }

    fn is_vaa_consumed(
        env: Env,
        emitter_chain: u32,
        emitter_address: BytesN<32>,
        sequence: u64,
    ) -> bool {
        inbound::is_vaa_consumed(&env, emitter_chain, &emitter_address, sequence)
    }

    fn receive_vaa(env: Env, vaa_bytes: Bytes) -> Result<(), TransceiverError> {
        Self::receive_message(env, vaa_bytes)
    }
}
