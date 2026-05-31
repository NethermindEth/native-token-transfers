//! Wormhole VAA assembly + secp256k1 signing, plus the Stellar-address →
//! 32-byte conversion used in NTT message recipient fields.
//!
//! The harness signs VAAs host-side as the test guardian, so inbound
//! integration tests can submit messages with the same wire format real
//! guardian signatures would produce. [`craft_body`] builds the VAA body
//! that gets keccak-then-keccak hashed and signed by [`sign`];
//! [`assemble`] prepends the version byte, guardian-set index, and
//! signature list to produce the final VAA bytes.

use secp256k1::{
    ecdsa::RecoverableSignature, Message, PublicKey, SecretKey,
};
use stellar_strkey::Strkey;
use tiny_keccak::{Hasher, Keccak};

/// Decoded Stellar strkey converted to its raw 32-byte payload — the
/// representation NTT messages use for cross-chain addresses. Accepts both
/// G-account public keys and C-contract hashes; panics for other strkey
/// types (muxed accounts, secret seeds, etc.).
pub fn stellar_addr_to_bytes32(addr: &str) -> [u8; 32] {
    match Strkey::from_string(addr).expect("invalid Stellar strkey") {
        Strkey::PublicKeyEd25519(pk) => pk.0,
        Strkey::Contract(c) => c.0,
        other => panic!("unsupported Stellar address type: {other:?}"),
    }
}

/// One guardian's signature over a VAA body, in the wire layout
/// [`assemble`] expects: 1-byte guardian index, 64-byte compact signature,
/// 1-byte recovery id.
pub struct GuardianSignature {
    pub index: u8,
    pub sig: [u8; 64],
    pub recovery_id: u8,
}

/// Inputs to [`craft_body`]. Mirrors the Wormhole VAA body layout 1:1.
pub struct VaaBodyInputs<'a> {
    pub timestamp: u32,
    pub nonce: u32,
    pub emitter_chain: u16,
    pub emitter_address: [u8; 32],
    pub sequence: u64,
    pub consistency_level: u8,
    pub payload: &'a [u8],
}

/// Keccak-256 hash of `data`.
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

/// Derives the 20-byte Ethereum-style address that identifies a guardian on
/// the wire. Used to seed the Wormhole core's initial guardian set so the
/// signatures [`sign`] produces verify against the expected address.
pub fn eth_address_from_privkey(privkey: &[u8; 32]) -> [u8; 20] {
    let sk = SecretKey::from_secret_bytes(*privkey)
        .expect("invalid secp256k1 secret");
    let pk = PublicKey::from_secret_key(&sk);
    let pk_uncompressed = pk.serialize_uncompressed();
    let hash = keccak256(&pk_uncompressed[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

/// Builds the Wormhole VAA body bytes for `inputs`. The body is what
/// [`sign`] hashes; it does not include the version byte, guardian-set
/// index, or signatures (those come from [`assemble`]).
pub fn craft_body(inputs: &VaaBodyInputs<'_>) -> Vec<u8> {
    let mut body = Vec::with_capacity(51 + inputs.payload.len());
    body.extend_from_slice(&inputs.timestamp.to_be_bytes());
    body.extend_from_slice(&inputs.nonce.to_be_bytes());
    body.extend_from_slice(&inputs.emitter_chain.to_be_bytes());
    body.extend_from_slice(&inputs.emitter_address);
    body.extend_from_slice(&inputs.sequence.to_be_bytes());
    body.push(inputs.consistency_level);
    body.extend_from_slice(inputs.payload);
    body
}

/// Signs the keccak-then-keccak hash of `body` with `privkey` (the
/// Wormhole guardian signing convention).
pub fn sign(body: &[u8], privkey: &[u8; 32], guardian_index: u8) -> GuardianSignature {
    let body_hash = keccak256(&keccak256(body));
    let sk = SecretKey::from_secret_bytes(*privkey)
        .expect("invalid secp256k1 secret");
    let msg = Message::from_digest(body_hash);
    let sig = RecoverableSignature::sign_ecdsa_recoverable(msg, &sk);
    let (recid, compact) = sig.serialize_compact();
    GuardianSignature {
        index: guardian_index,
        sig: compact,
        recovery_id: recid.to_u8(),
    }
}

/// Concatenates the VAA envelope (version, guardian-set index, signature
/// count, signatures) with `body` to produce the bytes the Wormhole core
/// accepts on `verify_vaa`.
pub fn assemble(
    guardian_set_index: u32,
    sigs: &[GuardianSignature],
    body: &[u8],
) -> Vec<u8> {
    let mut vaa = Vec::with_capacity(6 + sigs.len() * 66 + body.len());
    vaa.push(1);
    vaa.extend_from_slice(&guardian_set_index.to_be_bytes());
    vaa.push(sigs.len() as u8);
    for sig in sigs {
        vaa.push(sig.index);
        vaa.extend_from_slice(&sig.sig);
        vaa.push(sig.recovery_id);
    }
    vaa.extend_from_slice(body);
    vaa
}
