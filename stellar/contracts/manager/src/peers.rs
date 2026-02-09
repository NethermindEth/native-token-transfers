//! Peer management for cross-chain NTT Manager connections.
//!
//! Each peer represents an NTT Manager on another chain. Peers are identified
//! by their Wormhole chain ID and store the remote manager's address, token
//! decimals, and an independent inbound rate limit.

use soroban_sdk::{BytesN, Env};

use crate::{
    errors::NttManagerError,
    rate_limit::{RateLimitParams, RateLimitResult},
    state::NttManagerPeer,
    storage::{InstanceStorage, PeerEntry},
};

/// Retrieves peer configuration for a given chain.
///
/// Returns `None` if no peer is registered for the chain ID.
pub fn get_peer(env: &Env, chain_id: u32) -> Option<NttManagerPeer> {
    PeerEntry::new(env, chain_id).get()
}

/// Registers or updates a peer NTT Manager.
///
/// If the peer already exists, updates its address, decimals, and adjusts the
/// rate limit proportionally. New peers start with full inbound capacity.
///
/// # Errors
/// - `InvalidPeerChainIdZero` if chain_id is 0
/// - `InvalidPeerSameChainId` if chain_id matches this contract's chain
/// - `InvalidPeerZeroAddress` if address is all zeros
/// - `InvalidPeerDecimals` if decimals is 0 or exceeds 18
/// - `NotInitialized` if the contract hasn't been initialized
pub fn set_peer(
    env: &Env,
    chain_id: u32,
    address: BytesN<32>,
    token_decimals: u32,
    inbound_limit: u64,
) -> Result<(), NttManagerError> {
    if chain_id == 0 {
        return Err(NttManagerError::InvalidPeerChainIdZero);
    }

    // TODO: Implement as validation function in core contract interface
    if chain_id > u16::MAX as u32 {
        return Err(NttManagerError::ChainIdTooLarge);
    }

    let storage = InstanceStorage::new(env);
    let our_chain_id = storage.chain_id()?;

    if chain_id == our_chain_id {
        return Err(NttManagerError::InvalidPeerSameChainId);
    }

    let zero_address = BytesN::from_array(env, &[0u8; 32]);
    if address == zero_address {
        return Err(NttManagerError::InvalidPeerZeroAddress);
    }

    if token_decimals == 0 || token_decimals > 18 {
        return Err(NttManagerError::InvalidPeerDecimals);
    }

    let entry = PeerEntry::new(env, chain_id);
    let peer = entry.get().map_or_else(
        || NttManagerPeer {
            address: address.clone(),
            token_decimals,
            inbound_rate_limit: RateLimitParams::new(inbound_limit, env),
        },
        |mut existing| {
            existing.address = address.clone();
            existing.token_decimals = token_decimals;
            existing.inbound_rate_limit.set_limit(inbound_limit, env);
            existing
        },
    );

    entry.set(&peer);
    Ok(())
}

/// Updates the inbound rate limit for an existing peer.
///
/// Adjusts capacity proportionally when changing the limit.
///
/// # Errors
/// - `PeerNotFound` if no peer is registered for the chain ID
pub fn set_inbound_limit(env: &Env, chain_id: u32, limit: u64) -> Result<(), NttManagerError> {
    let entry = PeerEntry::new(env, chain_id);
    let mut peer = entry.get_or_err()?;
    peer.inbound_rate_limit.set_limit(limit, env);
    entry.set(&peer);
    Ok(())
}

/// Validates that a message source matches the registered peer.
///
/// Used during inbound transfer processing to ensure messages originate
/// from the expected NTT Manager on the source chain.
///
/// # Errors
/// - `PeerNotFound` if no peer is registered for the chain ID
/// - `InvalidPeer` if the source address doesn't match the registered peer
pub fn verify_peer(
    env: &Env,
    chain_id: u32,
    source_address: &BytesN<32>,
) -> Result<(), NttManagerError> {
    let peer = PeerEntry::new(env, chain_id).get_or_err()?;
    if peer.address != *source_address {
        return Err(NttManagerError::InvalidPeer);
    }
    Ok(())
}

/// Retrieves the inbound rate limit parameters for a chain.
///
/// Returns `None` if no peer is registered for the chain ID.
#[allow(dead_code)]
pub fn get_inbound_rate_limit(env: &Env, chain_id: u32) -> Option<RateLimitParams> {
    PeerEntry::new(env, chain_id)
        .get()
        .map(|p| p.inbound_rate_limit)
}

/// Refills the inbound rate limit for a specific peer chain.
///
/// Called during outbound transfers to implement backflow: when tokens leave
/// this chain, inbound capacity from that chain increases. This maintains
/// balance and prevents rate limit deadlocks. Does nothing if no peer exists
/// for the chain ID.
pub fn refill_inbound(env: &Env, chain_id: u32, amount: u64) {
    let entry = PeerEntry::new(env, chain_id);
    if let Some(mut peer) = entry.get() {
        peer.inbound_rate_limit.refill(amount, env);
        entry.set(&peer);
    }
}

/// Attempts to consume inbound rate limit capacity for a transfer.
///
/// Checks if the peer's inbound rate limit can accommodate the transfer amount.
/// Returns `Consumed` if successful (capacity used), or `Delayed(timestamp)` if
/// the caller should queue. Only persists the updated capacity when consumed —
/// the peer is re-saved because the rate limit is nested within it.
///
/// # Errors
/// - `PeerNotFound` if no peer is registered for the chain ID
pub fn consume_or_delay_inbound(
    env: &Env,
    chain_id: u32,
    amount: u64,
) -> Result<RateLimitResult, NttManagerError> {
    let entry = PeerEntry::new(env, chain_id);
    let mut peer = entry.get_or_err()?;
    let result = peer.inbound_rate_limit.consume_or_delay(amount, env);

    if matches!(result, RateLimitResult::Consumed) {
        entry.set(&peer);
    }

    Ok(result)
}
