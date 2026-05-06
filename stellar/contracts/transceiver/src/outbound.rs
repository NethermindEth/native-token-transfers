use soroban_ntt_client::{validate_chain_id, TransceiverError, TransceiverMessage};
use soroban_sdk::{Bytes, BytesN, Env};
use wormhole_soroban_client::{ConsistencyLevel, WormholeClient};

use crate::flatten_call;
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

    flatten_call(
        WormholeClient::new(env, &storage.wormhole_core()?).try_post_message(
            &env.current_contract_address(),
            &0u32,
            &payload,
            &ConsistencyLevel::Confirmed,
        ),
        TransceiverError::WormholePostFailed,
    )?;
    Ok(())
}

pub fn quote_delivery_price(env: &Env, recipient_chain: u32) -> Result<i128, TransceiverError> {
    if validate_chain_id(recipient_chain).is_none() {
        return Err(TransceiverError::ChainIdTooLarge);
    }
    let core = InstanceStorage::new(env).wormhole_core()?;
    let fee = WormholeClient::new(env, &core)
        .try_get_message_fee()
        .map_err(|_| TransceiverError::WormholeQueryFailed)?
        .map_err(|_| TransceiverError::WormholeQueryFailed)?;
    Ok(fee as i128)
}
