use soroban_ntt_client::messages::{
    NativeTokenTransfer, NttManagerMessage, TransceiverMessage, TrimmedAmount,
};
use soroban_sdk::{Bytes, BytesN, Env};

pub struct NttManagerMessageInputs {
    pub id: [u8; 32],
    pub sender: [u8; 32],
    pub source_token: [u8; 32],
    pub recipient: [u8; 32],
    pub recipient_chain: u32,
    pub trimmed_amount: u64,
    pub trimmed_decimals: u32,
}

pub fn encode_ntt_manager_message(inputs: &NttManagerMessageInputs) -> Vec<u8> {
    let env = Env::default();
    let msg = NttManagerMessage {
        id: BytesN::from_array(&env, &inputs.id),
        sender: BytesN::from_array(&env, &inputs.sender),
        payload: NativeTokenTransfer {
            amount: TrimmedAmount {
                amount: inputs.trimmed_amount,
                decimals: inputs.trimmed_decimals,
            },
            source_token: BytesN::from_array(&env, &inputs.source_token),
            to: BytesN::from_array(&env, &inputs.recipient),
            to_chain: inputs.recipient_chain,
            additional_payload: None,
        },
    };
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
