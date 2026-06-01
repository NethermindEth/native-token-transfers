use soroban_sdk::{Bytes, BytesN, Env, Vec as SorobanVec};
use vaa_test_signing::{assemble, sign};
use wormhole_soroban_client::{ConsistencyLevel, VAA};

use crate::common::guardian::GUARDIAN_SECRET;

/// Builds a VAA the in-process Wormhole core will verify and parse: it wraps
/// `payload` (the transceiver-message bytes) in a body emitted by
/// `(emitter_chain, emitter_address, sequence)`, then signs it with the test
/// guardian.
pub fn signed_inbound_vaa(
    env: &Env,
    emitter_chain: u16,
    emitter_address: &BytesN<32>,
    sequence: u64,
    payload: &Bytes,
) -> Bytes {
    let body = serialize_body(env, emitter_chain, emitter_address, sequence, payload);
    Bytes::from_slice(env, &assemble(0, &[sign(&body, &GUARDIAN_SECRET, 0)], &body))
}

/// Same as [`signed_inbound_vaa`] but flips one signature byte so the core's
/// `parse_and_verify_vaa` rejects it — the inbound path where verification
/// itself is the active barrier.
pub fn tampered_inbound_vaa(
    env: &Env,
    emitter_chain: u16,
    emitter_address: &BytesN<32>,
    sequence: u64,
    payload: &Bytes,
) -> Bytes {
    let body = serialize_body(env, emitter_chain, emitter_address, sequence, payload);
    let mut bad = sign(&body, &GUARDIAN_SECRET, 0);
    bad.sig[0] ^= 0xFF;
    Bytes::from_slice(env, &assemble(0, &[bad], &body))
}

fn serialize_body(
    env: &Env,
    emitter_chain: u16,
    emitter_address: &BytesN<32>,
    sequence: u64,
    payload: &Bytes,
) -> std::vec::Vec<u8> {
    VAA {
        version: 1,
        guardian_set_index: 0,
        signatures: SorobanVec::new(env),
        timestamp: 0,
        nonce: 0,
        emitter_chain: u32::from(emitter_chain),
        emitter_address: emitter_address.clone(),
        sequence,
        consistency_level: ConsistencyLevel::Confirmed,
        payload: payload.clone(),
    }
    .serialize_body(env)
    .iter()
    .collect()
}
