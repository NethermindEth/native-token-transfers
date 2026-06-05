use soroban_ntt_client::TransceiverMessage;
use soroban_sdk::{Bytes, BytesN, Env};

/// Builds the transceiver-envelope bytes a verified VAA carries: the
/// `TransceiverMessage` wrapping `manager_payload`, addressed from
/// `source_manager` to `recipient_manager`, with an empty transceiver payload.
pub fn build_transceiver_payload(
    env: &Env,
    source_manager: &BytesN<32>,
    recipient_manager: &BytesN<32>,
    manager_payload: &Bytes,
) -> Bytes {
    TransceiverMessage {
        source_manager: source_manager.clone(),
        recipient_manager: recipient_manager.clone(),
        manager_payload: manager_payload.clone(),
        transceiver_payload: Bytes::new(env),
    }
    .to_bytes(env)
    .unwrap()
}
