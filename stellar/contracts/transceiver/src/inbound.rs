use soroban_ntt_client::{NttManagerClient, TransceiverError, TransceiverMessage};
use soroban_sdk::{Address, Bytes, BytesN, Env};
use wormhole_soroban_client::WormholeClient;

use crate::peers::load_enabled_peer;
use crate::storage::{ConsumedEntry, InstanceStorage};

pub fn receive_message(env: &Env, vaa_bytes: Bytes) -> Result<(), TransceiverError> {
    let storage = InstanceStorage::new(env);

    let vaa = WormholeClient::new(env, &storage.wormhole_core()?)
        .try_parse_and_verify_vaa(&vaa_bytes)
        .map_err(|_| TransceiverError::WormholeVerificationFailed)?
        .map_err(|_| TransceiverError::WormholeVerificationFailed)?;

    let peer = load_enabled_peer(env, vaa.emitter_chain)?;
    if peer.emitter != vaa.emitter_address {
        return Err(TransceiverError::UnexpectedEmitter);
    }

    let decoded = TransceiverMessage::from_bytes(env, &vaa.payload)?;
    if decoded.recipient_manager != storage.manager_id()? {
        return Err(TransceiverError::UnexpectedRecipientManager);
    }

    let consumed = ConsumedEntry::new(env, vaa.emitter_chain, &vaa.emitter_address, vaa.sequence);
    if consumed.is_consumed() {
        return Err(TransceiverError::ReplayDetected);
    }
    consumed.mark_consumed();

    forward_to_manager(
        env,
        &storage.manager()?,
        vaa.emitter_chain,
        decoded.source_manager,
        decoded.manager_payload,
    )
}

pub fn is_vaa_consumed(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) -> bool {
    ConsumedEntry::new(env, emitter_chain, emitter_address, sequence).is_consumed()
}

fn forward_to_manager(
    env: &Env,
    manager: &Address,
    source_chain: u32,
    source_manager: BytesN<32>,
    payload: Bytes,
) -> Result<(), TransceiverError> {
    NttManagerClient::new(env, manager)
        .try_attestation_received(
            &env.current_contract_address(),
            &source_chain,
            &source_manager,
            &payload,
        )
        .map_err(|_| TransceiverError::ManagerRejectedMessage)?
        .map_err(|_| TransceiverError::ManagerRejectedMessage)?;

    Ok(())
}
