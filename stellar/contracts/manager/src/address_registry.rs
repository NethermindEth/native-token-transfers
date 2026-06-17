//! Manager-local registry mapping a Stellar address to and from its canonical
//! 32-byte Wormhole identity.
//!
//! Wormhole carries addresses as 32 one-way-hashed bytes, so an inbound
//! recipient can only be reconstructed by looking up an entry recorded earlier.

use soroban_ntt_client::NttManagerError;
use soroban_sdk::{Address, BytesN, Env};
use wormhole_soroban_client::hash_address;

use crate::storage::AddressEntry;

/// Records `address` under its canonical hash and returns that hash.
///
/// Permissionless: the key is `hash_address(address)`, so the entry is always
/// self-consistent and cannot be poisoned by another caller. Recipients and
/// relayers must be able to register before an inbound transfer is redeemed.
pub fn record_address(env: &Env, address: &Address) -> BytesN<32> {
    let hash = hash_address(env, address);
    AddressEntry::new(env, hash.clone()).set(address);
    hash
}

/// Resolves a previously recorded hash to its `Address`, extending TTL on hit.
pub fn resolve_address(env: &Env, hash: &BytesN<32>) -> Result<Address, NttManagerError> {
    AddressEntry::new(env, hash.clone()).get_or_err()
}
