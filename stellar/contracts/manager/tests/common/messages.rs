use soroban_ntt_client::{NativeTokenTransfer, NttManagerMessage, TrimmedAmount};
use soroban_sdk::{BytesN, Env};

/// Builds an inbound NTT message carrying `amount` (at `decimals`) destined for
/// `to` on `to_chain`. `id` fills the 32-byte message id so distinct messages
/// produce distinct digests.
pub fn ntt_message(
    env: &Env,
    id: u8,
    amount: u64,
    decimals: u8,
    to: &BytesN<32>,
    to_chain: u32,
) -> NttManagerMessage {
    NttManagerMessage {
        id: BytesN::from_array(env, &[id; 32]),
        sender: BytesN::from_array(env, &[0xAB; 32]),
        payload: NativeTokenTransfer {
            amount: TrimmedAmount::new(amount, decimals).unwrap(),
            source_token: BytesN::from_array(env, &[0xCD; 32]),
            to: to.clone(),
            to_chain,
            additional_payload: None,
        },
    }
}
