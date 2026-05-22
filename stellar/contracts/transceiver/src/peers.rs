use soroban_ntt_client::{validate_chain_id, PeerInfo, TransceiverError};
use soroban_sdk::{BytesN, Env};

use crate::storage::{InstanceStorage, PeerEntry};

/// Loads a registered, enabled peer for the given chain.
pub fn load_enabled_peer(env: &Env, chain_id: u32) -> Result<PeerInfo, TransceiverError> {
    let peer = PeerEntry::new(env, chain_id)
        .get()
        .ok_or(TransceiverError::PeerNotFound)?;
    if !peer.enabled {
        return Err(TransceiverError::PeerDisabled);
    }
    Ok(peer)
}

fn validate_input(chain_id: u32, emitter: &BytesN<32>) -> Result<(), TransceiverError> {
    if chain_id == 0 {
        return Err(TransceiverError::InvalidPeerChainIdZero);
    }
    if validate_chain_id(chain_id).is_none() {
        return Err(TransceiverError::ChainIdTooLarge);
    }
    if emitter.to_array() == [0u8; 32] {
        return Err(TransceiverError::InvalidPeerZeroAddress);
    }
    Ok(())
}

pub fn set_peer(
    env: &Env,
    chain_id: u32,
    emitter: BytesN<32>,
) -> Result<(), TransceiverError> {
    InstanceStorage::new(env).require_admin_auth()?;
    validate_input(chain_id, &emitter)?;

    let entry = PeerEntry::new(env, chain_id);
    if entry.get().is_some() {
        return Err(TransceiverError::PeerAlreadySet);
    }
    entry.set(&PeerInfo { emitter, enabled: true });
    Ok(())
}

pub fn update_peer(
    env: &Env,
    chain_id: u32,
    emitter: BytesN<32>,
) -> Result<(), TransceiverError> {
    InstanceStorage::new(env).require_admin_auth()?;
    validate_input(chain_id, &emitter)?;

    let entry = PeerEntry::new(env, chain_id);
    let mut info = entry.get().ok_or(TransceiverError::PeerNotFound)?;
    info.emitter = emitter;
    entry.set(&info);
    Ok(())
}

pub fn set_peer_enabled(
    env: &Env,
    chain_id: u32,
    enabled: bool,
) -> Result<(), TransceiverError> {
    InstanceStorage::new(env).require_admin_auth()?;

    let entry = PeerEntry::new(env, chain_id);
    let mut info = entry.get().ok_or(TransceiverError::PeerNotFound)?;
    info.enabled = enabled;
    entry.set(&info);
    Ok(())
}

pub fn get_peer(env: &Env, chain_id: u32) -> Option<BytesN<32>> {
    PeerEntry::new(env, chain_id).get().map(|info| info.emitter)
}

pub fn get_peer_info(env: &Env, chain_id: u32) -> Option<PeerInfo> {
    PeerEntry::new(env, chain_id).get()
}

pub fn is_peer_enabled(env: &Env, chain_id: u32) -> bool {
    PeerEntry::new(env, chain_id)
        .get()
        .is_some_and(|info| info.enabled)
}
