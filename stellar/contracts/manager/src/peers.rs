//! Peer management for cross-chain NTT Manager connections.
//!
//! Each peer represents an NTT Manager on another chain. Peers are identified
//! by their Wormhole chain ID and store the remote manager's address, token
//! decimals, and an independent inbound rate limit.

use soroban_sdk::{contracttype, BytesN, Env};

use crate::errors::NttManagerError;
use crate::rate_limit::RateLimitParams;
use crate::state::DataKey;

/// Peer NTT Manager on another chain.
///
/// Each peer maintains its own inbound rate limit, allowing independent
/// throttling of transfers from different source chains.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NttManagerPeer {
    /// 32-byte address of the NTT Manager on the peer chain.
    pub address: BytesN<32>,
    /// Token decimals on the peer chain (1-18). Used for amount normalization.
    pub token_decimals: u32,
    /// Rate limiter for inbound transfers from this chain.
    pub inbound_rate_limit: RateLimitParams,
}

/// Retrieves peer configuration for a given chain.
///
/// Returns `None` if no peer is registered for the chain ID.
pub fn get_peer(env: &Env, chain_id: u32) -> Option<NttManagerPeer> {
    env.storage().persistent().get(&DataKey::Peer(chain_id))
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

    let our_chain_id: u32 = env
        .storage()
        .instance()
        .get(&DataKey::ChainId)
        .ok_or(NttManagerError::NotInitialized)?;

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

    let existing: Option<NttManagerPeer> = get_peer(env, chain_id);

    let peer = if let Some(mut existing_peer) = existing {
        existing_peer.address = address;
        existing_peer.token_decimals = token_decimals;
        existing_peer
            .inbound_rate_limit
            .set_limit(inbound_limit, env);
        existing_peer
    } else {
        NttManagerPeer {
            address,
            token_decimals,
            inbound_rate_limit: RateLimitParams::new(inbound_limit, env),
        }
    };

    env.storage()
        .persistent()
        .set(&DataKey::Peer(chain_id), &peer);

    Ok(())
}

/// Updates the inbound rate limit for an existing peer.
///
/// Adjusts capacity proportionally when changing the limit.
///
/// # Errors
/// - `PeerNotFound` if no peer is registered for the chain ID
pub fn set_inbound_limit(env: &Env, chain_id: u32, limit: u64) -> Result<(), NttManagerError> {
    let mut peer: NttManagerPeer = get_peer(env, chain_id).ok_or(NttManagerError::PeerNotFound)?;

    peer.inbound_rate_limit.set_limit(limit, env);

    env.storage()
        .persistent()
        .set(&DataKey::Peer(chain_id), &peer);

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
    let peer = get_peer(env, chain_id).ok_or(NttManagerError::PeerNotFound)?;

    if peer.address != *source_address {
        return Err(NttManagerError::InvalidPeer);
    }

    Ok(())
}

/// Retrieves the inbound rate limit parameters for a chain.
///
/// Returns `None` if no peer is registered for the chain ID.
pub fn get_inbound_rate_limit(env: &Env, chain_id: u32) -> Option<RateLimitParams> {
    get_peer(env, chain_id).map(|p| p.inbound_rate_limit)
}

/// Refills the inbound rate limit for a specific peer chain.
///
/// Called during outbound transfers to implement backflow: when tokens leave
/// this chain, inbound capacity from that chain increases. This maintains
/// balance and prevents rate limit deadlocks. Does nothing if no peer exists
/// for the chain ID.
pub fn refill_inbound(env: &Env, chain_id: u32, amount: u64) {
    if let Some(mut peer) = get_peer(env, chain_id) {
        peer.inbound_rate_limit.refill(amount, env);
        env.storage()
            .persistent()
            .set(&DataKey::Peer(chain_id), &peer);
    }
}
