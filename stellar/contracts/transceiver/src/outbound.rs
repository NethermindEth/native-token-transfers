use soroban_ntt_client::{
    address_to_bytes32, emit_message_sent, flatten_call, validate_chain_id, NttManagerClient,
    TransceiverError, TransceiverMessage, WormholeTransceiverInfo, WormholeTransceiverRegistration,
};
use soroban_sdk::{Bytes, BytesN, Env};
use wormhole_soroban_client::{ConsistencyLevel, WormholeClient};

use crate::peers::{get_peer_info, load_enabled_peer};
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
        recipient_manager: recipient_manager.clone(),
        manager_payload: manager_payload.clone(),
        transceiver_payload: Bytes::new(env),
    }
    .to_bytes(env)?;

    post_message(env, payload)?;
    emit_message_sent(env, recipient_chain, &recipient_manager, &manager_payload);
    Ok(())
}

pub fn quote_delivery_price(env: &Env) -> Result<i128, TransceiverError> {
    let core = InstanceStorage::new(env).wormhole_core()?;
    let fee = flatten_call(
        WormholeClient::new(env, &core).try_get_message_fee(),
        TransceiverError::WormholeQueryFailed,
    )?;
    Ok(fee as i128)
}

pub fn broadcast_id(env: &Env) -> Result<(), TransceiverError> {
    let manager = InstanceStorage::new(env).manager()?;
    let manager_client = NttManagerClient::new(env, &manager);
    let err = TransceiverError::ManagerQueryFailed;
    let manager_mode = flatten_call(manager_client.try_get_mode(), err)?;
    let token = flatten_call(manager_client.try_get_token(), err)?;
    let token_decimals = flatten_call(manager_client.try_token_decimals(), err)?;

    let payload = WormholeTransceiverInfo {
        manager_address: address_to_bytes32(&manager),
        manager_mode,
        token_address: address_to_bytes32(&token),
        token_decimals: token_decimals as u8,
    }
    .to_bytes(env);

    post_message(env, payload)
}

pub fn broadcast_peer(env: &Env, chain_id: u32) -> Result<(), TransceiverError> {
    let chain_id_u16 = validate_chain_id(chain_id).ok_or(TransceiverError::ChainIdTooLarge)?;
    let peer = get_peer_info(env, chain_id).ok_or(TransceiverError::PeerNotFound)?;

    let payload = WormholeTransceiverRegistration {
        chain_id: chain_id_u16,
        transceiver_address: peer.emitter,
    }
    .to_bytes(env);

    post_message(env, payload)
}

fn post_message(env: &Env, payload: Bytes) -> Result<(), TransceiverError> {
    let core = InstanceStorage::new(env).wormhole_core()?;
    flatten_call(
        WormholeClient::new(env, &core).try_post_message(
            &env.current_contract_address(),
            &0u32,
            &payload,
            &ConsistencyLevel::Confirmed,
        ),
        TransceiverError::WormholePostFailed,
    )?;
    Ok(())
}
