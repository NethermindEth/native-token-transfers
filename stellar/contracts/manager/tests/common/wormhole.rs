use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};
use wormhole_soroban_client::{hash_address, WormholeError};

/// Stand-in for the Wormhole core address registry: `record_address` stores
/// `hash_address(addr) -> addr`, `get_address_from_hash` resolves it. The
/// manager resolves inbound recipients through this in tests.
#[contract]
pub struct MockWormholeCore;

#[contractimpl]
impl MockWormholeCore {
    pub fn record_address(env: Env, address: Address) -> Result<BytesN<32>, WormholeError> {
        let hash = hash_address(&env, &address);
        env.storage().persistent().set(&hash, &address);
        Ok(hash)
    }

    pub fn get_address_from_hash(env: Env, hash: BytesN<32>) -> Result<Address, WormholeError> {
        env.storage()
            .persistent()
            .get(&hash)
            .ok_or(WormholeError::AddressNotFound)
    }
}

/// Wormhole core whose lookup always fails with a non-`AddressNotFound` error,
/// standing in for an unreachable or misconfigured core — exercises the inbound
/// resolution path that must surface as `WormholeCoreCallFailed`.
#[contract]
pub struct FaultyWormholeCore;

#[contractimpl]
impl FaultyWormholeCore {
    pub fn get_address_from_hash(_env: Env, _hash: BytesN<32>) -> Result<Address, WormholeError> {
        Err(WormholeError::StorageError)
    }
}
