use soroban_ntt_client::{TransceiverError, TransceiverMessage};
use soroban_sdk::{vec, Address, Bytes, BytesN, Env, IntoVal, Symbol};
use wormhole_soroban_client::{ConsistencyLevel, WormholeError};

use crate::peers::load_enabled_peer;
use crate::storage::InstanceStorage;

pub fn send_message(
    env: &Env,
    recipient_chain: u32,
    recipient_manager: BytesN<32>,
    manager_payload: Bytes,
) -> Result<(), TransceiverError> {
    let storage = InstanceStorage::new(env);
    storage.require_manager_auth()?;
    load_enabled_peer(env, recipient_chain)?;

    let payload = TransceiverMessage {
        source_manager: storage.manager_id()?,
        recipient_manager,
        manager_payload,
        transceiver_payload: Bytes::new(env),
    }
    .to_bytes(env)?;

    post_to_wormhole(env, &storage.wormhole_core()?, payload)
}

fn post_to_wormhole(env: &Env, core: &Address, payload: Bytes) -> Result<(), TransceiverError> {
    let args = vec![
        env,
        env.current_contract_address().into_val(env),
        0u32.into_val(env),
        payload.into_val(env),
        ConsistencyLevel::Confirmed.into_val(env),
    ];
    let res: Result<u64, WormholeError> =
        env.invoke_contract(core, &Symbol::new(env, "post_message"), args);
    res.map(|_| ())
        .map_err(|_| TransceiverError::WormholePostFailed)
}
