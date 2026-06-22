#![cfg(test)]
//! Pins `hash_address` (git-pinned via `wormhole-soroban-client`) to keccak256
//! of an address's StrKey text, for both `G…` and `C…` kinds. Fails if a dep
//! bump changes the hash input back to raw payload bytes.

use soroban_sdk::{Address, Bytes, Env, String};
use wormhole_soroban_client::hash_address;

const ACCOUNT: &str = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";
const CONTRACT: &str = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";

#[test]
fn hash_address_is_keccak256_of_strkey_text() {
    let env = Env::default();
    for strkey in [ACCOUNT, CONTRACT] {
        let address = Address::from_string(&String::from_str(&env, strkey));
        // keccak the literal StrKey bytes, not `address.to_string()`, so a
        // changed hash input fails here instead of being mirrored.
        let expected = env
            .crypto()
            .keccak256(&Bytes::from_slice(&env, strkey.as_bytes()))
            .to_bytes();
        assert_eq!(hash_address(&env, &address), expected);
    }
}
