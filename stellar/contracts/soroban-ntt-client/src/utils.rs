//! Cross-contract conversion and validation helpers.

use soroban_sdk::{address_payload::AddressPayload, Address, BytesN, Env};

/// Converts a Soroban [`Address`] to its 32-byte cross-chain representation.
///
/// Extracts the underlying bytes from either an Ed25519 account public key
/// or a contract ID hash — both already 32 bytes wide.
///
/// # Panics
/// Panics only if the address has no payload, which the SDK does not
/// produce for valid addresses.
pub fn address_to_bytes32(address: &Address) -> BytesN<32> {
    match address.to_payload().expect("address has no payload") {
        AddressPayload::AccountIdPublicKeyEd25519(bytes) => bytes,
        AddressPayload::ContractIdHash(bytes) => bytes,
    }
}

/// Reconstructs a Soroban [`Address`] from its 32-byte cross-chain form.
///
/// Treats the bytes as an Ed25519 account public key, matching how
/// inbound NTT messages encode recipients.
pub fn bytes32_to_address(env: &Env, bytes: &BytesN<32>) -> Address {
    Address::from_payload(
        env,
        AddressPayload::AccountIdPublicKeyEd25519(bytes.clone()),
    )
}

/// Encodes a manager sequence number as the canonical 32-byte message ID.
///
/// Big-endian `u64` placed in the last 8 bytes; the leading 24 bytes are
/// zero-padded.
pub fn sequence_to_message_id(env: &Env, sequence: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&sequence.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

/// Narrows a Wormhole chain identifier to its protocol-defined `u16` range.
///
/// Wormhole chain IDs are 16-bit by spec; values above `u16::MAX` are
/// rejected at the boundary so downstream code can rely on the narrowed
/// type. Returns `None` when the value does not fit, leaving the choice
/// of error variant to the caller.
pub fn validate_chain_id(chain_id: u32) -> Option<u16> {
    u16::try_from(chain_id).ok()
}

/// Flattens a Soroban `try_X` client result, mapping any failure to `err`.
///
/// `try_X` methods return `Result<Result<T, E1>, E2>`: the outer `Result`
/// reports invocation-level failures, the inner reports a contract error
/// or host error. Both layers collapse to a single error variant chosen
/// by the caller.
pub fn flatten_call<T, E1, E2, Err>(
    r: Result<Result<T, E1>, E2>,
    err: Err,
) -> Result<T, Err> {
    match r {
        Ok(Ok(v)) => Ok(v),
        _ => Err(err),
    }
}
