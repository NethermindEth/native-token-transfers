#![no_std]

mod inbound;
mod outbound;
mod peers;
mod state;
mod storage;

use soroban_ntt_client::{
    PeerInfo, TransceiverError, TransceiverInterface, WormholeTransceiverInterface,
};
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};

use state::TRANSCEIVER_TYPE;
use storage::InstanceStorage;

/// Flattens a Soroban `try_X` client result, mapping any failure to `err`.
///
/// `try_X` returns `Result<Result<T, E1>, E2>` (outer = invocation, inner =
/// contract error vs. host error). The transceiver collapses both layers
/// to a single `TransceiverError` variant chosen by the caller.
pub(crate) fn flatten_call<T, E1, E2>(
    r: Result<Result<T, E1>, E2>,
    err: TransceiverError,
) -> Result<T, TransceiverError> {
    match r {
        Ok(Ok(v)) => Ok(v),
        _ => Err(err),
    }
}

#[contract]
pub struct TransceiverContract;

#[contractimpl]
impl TransceiverContract {
    pub fn __constructor(env: Env, admin: Address, manager: Address, wormhole_core: Address) {
        InstanceStorage::new(&env).initialize(&admin, &manager, &wormhole_core);
    }
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

    fn quote_delivery_price(env: Env, recipient_chain: u32) -> Result<i128, TransceiverError> {
        outbound::quote_delivery_price(&env, recipient_chain)
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
}
