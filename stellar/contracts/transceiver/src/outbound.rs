use soroban_ntt_client::{TransceiverError, TransceiverMessage};
use soroban_sdk::{Bytes, BytesN, Env};
use wormhole_soroban_client::{ConsistencyLevel, WormholeClient};

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

    WormholeClient::new(env, &storage.wormhole_core()?)
        .try_post_message(
            &env.current_contract_address(),
            &0u32,
            &payload,
            &ConsistencyLevel::Confirmed,
        )
        .map_err(|_| TransceiverError::WormholePostFailed)?
        .map_err(|_| TransceiverError::WormholePostFailed)?;

    Ok(())
}
