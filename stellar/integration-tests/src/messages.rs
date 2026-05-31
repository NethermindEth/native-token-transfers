use soroban_ntt_client::messages::{
    NativeTokenTransfer, NttManagerMessage, TransceiverMessage, TrimmedAmount,
};
use soroban_sdk::{Bytes, BytesN, Env};

use crate::vaa::{self, GuardianSignature, VaaBodyInputs};

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

pub struct InboundVaaInputs<'a> {
    pub ntt: NttManagerMessageInputs,
    pub source_manager: [u8; 32],
    pub recipient_manager: [u8; 32],
    pub emitter_chain: u16,
    pub emitter_address: [u8; 32],
    pub sequence: u64,
    pub guardian_secret: &'a [u8; 32],
}

pub fn build_inbound_vaa_hex(inputs: &InboundVaaInputs<'_>) -> String {
    let manager_payload = encode_ntt_manager_message(&inputs.ntt);
    let transceiver_payload = encode_transceiver_message(
        inputs.source_manager,
        inputs.recipient_manager,
        &manager_payload,
    );
    let body = vaa::craft_body(&VaaBodyInputs {
        timestamp: 0,
        nonce: 0,
        emitter_chain: inputs.emitter_chain,
        emitter_address: inputs.emitter_address,
        sequence: inputs.sequence,
        consistency_level: 1,
        payload: &transceiver_payload,
    });
    let sig: GuardianSignature = vaa::sign(&body, inputs.guardian_secret, 0);
    let assembled = vaa::assemble(0, std::slice::from_ref(&sig), &body);
    hex::encode(assembled)
}

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

pub fn encode_ntt_manager_message(inputs: &NttManagerMessageInputs) -> Vec<u8> {
    let env = Env::default();
    let msg = build_ntt_message(&env, inputs);
    let bytes = msg
        .to_bytes(&env)
        .expect("encode NttManagerMessage");
    bytes_to_vec(&bytes)
}

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

fn bytes_to_vec(b: &Bytes) -> Vec<u8> {
    b.iter().collect()
}
