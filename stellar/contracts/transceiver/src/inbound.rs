use soroban_ntt_client::{AttestationResult, NttManagerError, TransceiverError, TransceiverMessage};
use soroban_sdk::{vec, Address, Bytes, BytesN, Env, IntoVal, Symbol};
use wormhole_soroban_client::{WormholeError, VAA};

use crate::peers::load_enabled_peer;
use crate::storage::{ConsumedEntry, InstanceStorage};

pub fn receive_message(env: &Env, vaa_bytes: Bytes) -> Result<(), TransceiverError> {
    let storage = InstanceStorage::new(env);
    storage.require_initialized()?;

    let vaa = verify_and_parse_vaa(env, &storage.wormhole_core()?, vaa_bytes)?;

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

fn verify_and_parse_vaa(
    env: &Env,
    core: &Address,
    vaa_bytes: Bytes,
) -> Result<VAA, TransceiverError> {
    let verify_args = vec![env, vaa_bytes.clone().into_val(env)];
    let verified: Result<(), WormholeError> =
        env.invoke_contract(core, &Symbol::new(env, "verify_vaa"), verify_args);
    verified.map_err(|_| TransceiverError::WormholeVerificationFailed)?;

    let parse_args = vec![env, vaa_bytes.into_val(env)];
    let parsed: Result<VAA, WormholeError> =
        env.invoke_contract(core, &Symbol::new(env, "parse_vaa"), parse_args);
    parsed.map_err(|_| TransceiverError::WormholeParseFailed)
}

fn forward_to_manager(
    env: &Env,
    manager: &Address,
    source_chain: u32,
    source_manager: BytesN<32>,
    payload: Bytes,
) -> Result<(), TransceiverError> {
    let args = vec![
        env,
        env.current_contract_address().into_val(env),
        source_chain.into_val(env),
        source_manager.into_val(env),
        payload.into_val(env),
    ];
    let res: Result<AttestationResult, NttManagerError> =
        env.invoke_contract(manager, &Symbol::new(env, "attestation_received"), args);
    res.map(|_| ())
        .map_err(|_| TransceiverError::ManagerRejectedMessage)
}
