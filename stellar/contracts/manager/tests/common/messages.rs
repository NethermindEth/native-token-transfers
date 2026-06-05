use soroban_ntt_client::{NativeTokenTransfer, NttManagerMessage, TrimmedAmount};
use soroban_sdk::{Bytes, BytesN, Env};

/// Encodes an inbound NTT message (amount at `decimals`, destined for `to` on
/// `to_chain`) and returns its wire bytes and digest under `source_chain` — the
/// two forms the manager's inbound entrypoints consume.
pub fn ntt_message(
    env: &Env,
    amount: u64,
    decimals: u8,
    to: &BytesN<32>,
    to_chain: u32,
    source_chain: u32,
) -> (Bytes, BytesN<32>) {
    let message = NttManagerMessage {
        id: BytesN::from_array(env, &[1; 32]),
        sender: BytesN::from_array(env, &[0xAB; 32]),
        payload: NativeTokenTransfer {
            amount: TrimmedAmount::new(amount, decimals).unwrap(),
            source_token: BytesN::from_array(env, &[0xCD; 32]),
            to: to.clone(),
            to_chain,
            additional_payload: None,
        },
    };
    (
        message.to_bytes(env).unwrap(),
        message.compute_digest(env, source_chain as u16).unwrap(),
    )
}
