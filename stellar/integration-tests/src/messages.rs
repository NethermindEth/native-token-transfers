//! Host-side encoders for the NTT manager + transceiver wire messages, plus
//! [`build_inbound_vaa_hex`] which composes the full inbound VAA a test
//! wants to submit, and [`stellar_addr_to_hash`] for the canonical address
//! identity used on the wire.
//!
//! The contract-side encoders in `soroban-ntt-client` and the VAA body
//! serializer in `wormhole-soroban-client` both require a Soroban `Env` to
//! allocate, so the harness instantiates a `testutils` `Env` locally and
//! reuses them. This keeps those crates as the single source of truth for
//! the on-the-wire format — no hand-rolled byte layouts here.

use soroban_ntt_client::messages::{
    NativeTokenTransfer, NttManagerMessage, TransceiverMessage, TrimmedAmount,
};
use soroban_sdk::{Address, Bytes, BytesN, Env, String as SorobanString, Vec as SorobanVec};
use wormhole_soroban_client::{hash_address, ConsistencyLevel, VAA};

use crate::vaa::{self, GuardianSignature};

/// Field-level inputs for [`encode_ntt_manager_message`] and
/// [`compute_message_digest`]. Mirrors `NttManagerMessage` + the inner
/// `NativeTokenTransfer` payload using Rust-native primitive types so test
/// code doesn't need to construct Soroban types directly.
#[derive(Clone, Copy)]
pub struct NttManagerMessageInputs {
    pub id: [u8; 32],
    pub sender: [u8; 32],
    pub source_token: [u8; 32],
    pub recipient: [u8; 32],
    pub recipient_chain: u32,
    pub trimmed_amount: u64,
    pub trimmed_decimals: u32,
}

/// Inputs to [`build_inbound_vaa_hex`]. Bundles the NTT payload, the
/// transceiver envelope addresses, the VAA wormhole envelope fields, and
/// the guardian secret used to sign.
pub struct InboundVaaInputs<'a> {
    pub ntt: NttManagerMessageInputs,
    pub source_manager: [u8; 32],
    pub recipient_manager: [u8; 32],
    pub emitter_chain: u16,
    pub emitter_address: [u8; 32],
    pub sequence: u64,
    pub guardian_secret: &'a [u8; 32],
}

/// Computes the canonical 32-byte Wormhole identity of a Stellar address —
/// `keccak256(strkey)` via the shared on-chain [`hash_address`] — which every
/// Stellar-native field travels as on the wire (recipient `to`, the recipient
/// manager). The manager stores this hash under `record_address` and resolves
/// it on inbound, so a recipient must be registered (see `Stack::record_address`)
/// before a transfer to its hash can be redeemed.
pub fn stellar_addr_to_hash(addr: &str) -> [u8; 32] {
    let env = Env::default();
    let address = Address::from_string(&SorobanString::from_str(&env, addr));
    hash_address(&env, &address).to_array()
}

/// Composes the full inbound flow end-to-end: serializes the VAA body
/// (NTT manager message → transceiver message → VAA envelope), signs it
/// with the guardian secret, assembles the VAA, and hex-encodes it for the
/// CLI.
pub fn build_inbound_vaa_hex(inputs: &InboundVaaInputs<'_>) -> String {
    let body = serialize_inbound_vaa_body(inputs);
    let sig: GuardianSignature = vaa::sign(&body, inputs.guardian_secret, 0);
    let assembled = vaa::assemble(0, std::slice::from_ref(&sig), &body);
    hex::encode(assembled)
}

/// Same as [`build_inbound_vaa_hex`] but flips one byte of the guardian
/// signature so the real Wormhole core's verification rejects it. The only
/// way to exercise `WormholeVerificationFailed` at IT level — every other
/// inbound scenario submits a correctly-signed VAA.
pub fn build_inbound_vaa_hex_tampered_signature(inputs: &InboundVaaInputs<'_>) -> String {
    let body = serialize_inbound_vaa_body(inputs);
    let mut sig: GuardianSignature = vaa::sign(&body, inputs.guardian_secret, 0);
    sig.sig[0] ^= 0x01;
    let assembled = vaa::assemble(0, std::slice::from_ref(&sig), &body);
    hex::encode(assembled)
}

/// Serializes the VAA body shared by the signed and tampered builders:
/// the NTT manager message wrapped in a transceiver message, wrapped in the
/// VAA envelope, via `wormhole_soroban_client::VAA::serialize_body`. All
/// Soroban-side construction shares one `Env` so `Bytes` flows directly
/// between the layers without round-tripping through `Vec<u8>`.
fn serialize_inbound_vaa_body(inputs: &InboundVaaInputs<'_>) -> Vec<u8> {
    let env = Env::default();

    let manager_message = build_ntt_message(&env, &inputs.ntt);
    let manager_payload = manager_message
        .to_bytes(&env)
        .expect("encode NttManagerMessage");

    let transceiver_message = TransceiverMessage {
        source_manager: BytesN::from_array(&env, &inputs.source_manager),
        recipient_manager: BytesN::from_array(&env, &inputs.recipient_manager),
        manager_payload,
        transceiver_payload: Bytes::new(&env),
    };
    let transceiver_payload = transceiver_message
        .to_bytes(&env)
        .expect("encode TransceiverMessage");

    let vaa_for_body = VAA {
        version: 1,
        guardian_set_index: 0,
        signatures: SorobanVec::new(&env),
        timestamp: 0,
        nonce: 0,
        emitter_chain: u32::from(inputs.emitter_chain),
        emitter_address: BytesN::from_array(&env, &inputs.emitter_address),
        sequence: inputs.sequence,
        consistency_level: ConsistencyLevel::Confirmed,
        payload: transceiver_payload,
    };
    vaa_for_body.serialize_body(&env).iter().collect()
}

/// Returns the keccak-256 digest the manager computes on inbound for
/// deduplication and queue-key purposes: `keccak256(source_chain_be ||
/// NttManagerMessage_bytes)`.
pub fn compute_message_digest(
    inputs: &NttManagerMessageInputs,
    source_chain: u16,
) -> [u8; 32] {
    let env = Env::default();
    let msg = build_ntt_message(&env, inputs);
    let digest = msg
        .compute_digest(&env, source_chain)
        .expect("compute_digest");
    digest.to_array()
}

/// Returns the wire-encoded bytes of an `NttManagerMessage` built from
/// `inputs`. Used as the inner payload of a transceiver message and as
/// input to `manager.attestation_received` when bypassing the transceiver.
pub fn encode_ntt_manager_message(inputs: &NttManagerMessageInputs) -> Vec<u8> {
    let env = Env::default();
    let msg = build_ntt_message(&env, inputs);
    let bytes = msg
        .to_bytes(&env)
        .expect("encode NttManagerMessage");
    bytes_to_vec(&bytes)
}

/// Returns the wire-encoded bytes of a `TransceiverMessage` wrapping
/// `manager_payload`. The two manager addresses identify the source +
/// recipient NTT managers for the cross-chain transfer.
pub fn encode_transceiver_message(
    source_manager: [u8; 32],
    recipient_manager: [u8; 32],
    manager_payload: &[u8],
) -> Vec<u8> {
    let env = Env::default();
    let msg = TransceiverMessage {
        source_manager: BytesN::from_array(&env, &source_manager),
        recipient_manager: BytesN::from_array(&env, &recipient_manager),
        manager_payload: Bytes::from_slice(&env, manager_payload),
        transceiver_payload: Bytes::new(&env),
    };
    let bytes = msg
        .to_bytes(&env)
        .expect("encode TransceiverMessage");
    bytes_to_vec(&bytes)
}

fn build_ntt_message(env: &Env, inputs: &NttManagerMessageInputs) -> NttManagerMessage {
    NttManagerMessage {
        id: BytesN::from_array(env, &inputs.id),
        sender: BytesN::from_array(env, &inputs.sender),
        payload: NativeTokenTransfer {
            amount: TrimmedAmount {
                amount: inputs.trimmed_amount,
                decimals: inputs.trimmed_decimals,
            },
            source_token: BytesN::from_array(env, &inputs.source_token),
            to: BytesN::from_array(env, &inputs.recipient),
            to_chain: inputs.recipient_chain,
            additional_payload: None,
        },
    }
}

fn bytes_to_vec(b: &Bytes) -> Vec<u8> {
    b.iter().collect()
}
