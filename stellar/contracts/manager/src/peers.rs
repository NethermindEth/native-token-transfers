use soroban_sdk::{contracttype, BytesN};

use crate::rate_limit::RateLimitParams;

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NttManagerPeer {
    pub address: BytesN<32>,
    pub token_decimals: u32,
    pub inbound_rate_limit: RateLimitParams,
}

pub fn get_peer(env: &Env, chain_id: u32) -> Option<NttManagerPeer> {
    env.storage().persistent().get(&DataKey::Peer(chain_id))
}

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

pub fn set_inbound_limit(env: &Env, chain_id: u32, limit: u64) -> Result<(), NttManagerError> {
    let mut peer: NttManagerPeer = get_peer(env, chain_id).ok_or(NttManagerError::PeerNotFound)?;

    peer.inbound_rate_limit.set_limit(limit, env);

    env.storage()
        .persistent()
        .set(&DataKey::Peer(chain_id), &peer);

    Ok(())
}

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

pub fn get_inbound_rate_limit(env: &Env, chain_id: u32) -> Option<RateLimitParams> {
    get_peer(env, chain_id).map(|p| p.inbound_rate_limit)
}
