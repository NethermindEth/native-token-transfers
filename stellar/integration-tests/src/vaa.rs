use secp256k1::{
    ecdsa::RecoverableSignature, Message, PublicKey, SecretKey,
};
use stellar_strkey::Strkey;
use tiny_keccak::{Hasher, Keccak};

pub fn stellar_addr_to_bytes32(addr: &str) -> [u8; 32] {
    match Strkey::from_string(addr).expect("invalid Stellar strkey") {
        Strkey::PublicKeyEd25519(pk) => pk.0,
        Strkey::Contract(c) => c.0,
        other => panic!("unsupported Stellar address type: {other:?}"),
    }
}

pub struct GuardianSignature {
    pub index: u8,
    pub sig: [u8; 64],
    pub recovery_id: u8,
}

pub struct VaaBodyInputs<'a> {
    pub timestamp: u32,
    pub nonce: u32,
    pub emitter_chain: u16,
    pub emitter_address: [u8; 32],
    pub sequence: u64,
    pub consistency_level: u8,
    pub payload: &'a [u8],
}

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

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
