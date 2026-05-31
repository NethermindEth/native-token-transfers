//! Host-side guardian primitives: secp256k1 VAA signing and the
//! Ethereum-style guardian address derivation that pairs with it.
//!
//! Wormhole VAAs are produced off-chain by the guardian network in
//! production. Integration tests simulate one such guardian here so
//! they can submit signed VAAs to the on-chain Wormhole core for
//! verification. The VAA body itself is serialized via
//! `wormhole_soroban_client::VAA::serialize_body`; this module only
//! handles signing the body's hash and assembling the wire envelope
//! (version + guardian-set index + signatures + body) the core expects
//! on `parse_and_verify_vaa`.

use secp256k1::{
    ecdsa::RecoverableSignature, Message, PublicKey, SecretKey,
};
use tiny_keccak::{Hasher, Keccak};

/// One guardian's signature over a VAA body, laid out exactly as the
/// Wormhole wire format expects: 1-byte guardian index, 64-byte compact
/// signature, 1-byte recovery id.
pub struct GuardianSignature {
    pub index: u8,
    pub sig: [u8; 64],
    pub recovery_id: u8,
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

/// Signs the keccak-then-keccak hash of `body` with `privkey` — the Wormhole
/// guardian signing convention.
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
/// accepts on `parse_and_verify_vaa`.
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
