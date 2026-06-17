//! Pins the canonical `hash_address` NTT depends on to the exact bytes the
//! Wormhole core contract and the off-chain SDK produce.
//!
//! NTT consumes `hash_address` from `wormhole-soroban-client` (tracked as a git
//! dependency). Inbound `to` resolution only works if the bytes Stellar stores
//! under `record_address` match what peers and the SDK put on the wire, so a
//! dependency bump that silently changed the hash would misroute transfers.
//! These are the same vectors pinned in the Wormhole crate; they fail loudly if
//! the two ever diverge.

use soroban_sdk::{Address, BytesN, Env, String};
use wormhole_soroban_client::hash_address;

fn assert_vector(strkey: &str, expected: [u8; 32]) {
    let env = Env::default();
    let address = Address::from_string(&String::from_str(&env, strkey));
    assert_eq!(
        hash_address(&env, &address),
        BytesN::from_array(&env, &expected)
    );
}

#[test]
fn account_vector_matches_wormhole_client() {
    assert_vector(
        "GAIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCF6M",
        [
            0x27, 0x99, 0x72, 0xbd, 0x86, 0xb0, 0xe9, 0xcf, 0xf5, 0x3e, 0xd0, 0x68, 0xcc, 0x08,
            0x9b, 0x96, 0x59, 0x64, 0x75, 0x48, 0x6a, 0x38, 0x2e, 0xe5, 0x62, 0x95, 0x76, 0xb3,
            0x64, 0x12, 0x5d, 0x27,
        ],
    );
}

#[test]
fn contract_vector_matches_wormhole_client() {
    assert_vector(
        "CARCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEVQO",
        [
            0x79, 0xa0, 0x39, 0x9c, 0x82, 0x8a, 0xeb, 0x24, 0xb7, 0xf1, 0x70, 0x05, 0xd1, 0x29,
            0xf4, 0xa2, 0x95, 0xb3, 0xa6, 0x68, 0x68, 0x74, 0xcb, 0x1b, 0xea, 0xe5, 0xc9, 0xc9,
            0x5b, 0x95, 0xa9, 0x44,
        ],
    );
}
