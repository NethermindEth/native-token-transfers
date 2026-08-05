use executor_requests::make_ntt_v1_request;
use soroban_ntt_client::sequence_to_message_id;
use soroban_sdk::{Address, Bytes, Env};
use wormhole_soroban_client::hash_address;

/// Builds the ERN1 `request` payload that tells the Executor which NTT message
/// to deliver: `"ERN1" || srcChain(u16 BE) || srcManager(32) || messageId(32)`.
///
/// `srcManager` is the manager's 32-byte identifier (`hash_address`) and
/// `messageId` is the sequence right-aligned in 32 bytes — the same identifiers
/// the destination manager reconstructs when it verifies the delivered message.
pub fn ntt_request(env: &Env, chain_id: u16, ntt_manager: &Address, sequence: u64) -> Bytes {
    let request = make_ntt_v1_request(
        chain_id,
        hash_address(env, ntt_manager).to_array(),
        sequence_to_message_id(env, sequence).to_array(),
    );
    Bytes::from_slice(env, &request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    // Locks the full ERN1 wire layout: prefix, big-endian source chain, the
    // hashed source manager, and the sequence right-aligned in the 32-byte
    // message ID. Any drift in executor-requests' field order fails here.
    #[test]
    fn ntt_request_matches_ern1_layout() {
        let env = Env::default();
        let manager = Address::generate(&env);
        let seq = 0x0102_0304_0506_0708u64;

        let mut expected = Bytes::from_array(&env, b"ERN1");
        expected.extend_from_array(&10_002u16.to_be_bytes());
        expected.extend_from_array(&hash_address(&env, &manager).to_array());
        let mut message_id = [0u8; 32];
        message_id[24..].copy_from_slice(&seq.to_be_bytes());
        expected.extend_from_array(&message_id);

        assert_eq!(ntt_request(&env, 10_002, &manager, seq), expected);
    }
}
