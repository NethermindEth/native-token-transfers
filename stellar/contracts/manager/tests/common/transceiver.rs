use soroban_ntt_client::TransceiverError;
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env};
use stellar_ntt_manager::ManagerContractClient;

#[contracttype]
struct TransceiverConfig {
    quote: i128,
    fail_quote: bool,
    fail_send: bool,
}

/// What the manager passed to `send_message`, recorded so tests can assert the
/// manager built the correct outbound message.
#[contracttype]
pub struct SentMessage {
    pub recipient_chain: u32,
    pub recipient_manager: BytesN<32>,
    pub manager_payload: Bytes,
}

/// Minimal transceiver stand-in. The manager only reaches it through
/// `TransceiverClient`, so it implements just the methods the manager calls.
#[contract]
pub struct MockTransceiver;

#[contractimpl]
impl MockTransceiver {
    pub fn __constructor(env: Env, quote: i128, fail_quote: bool, fail_send: bool) {
        env.storage().instance().set(
            &symbol_short!("cfg"),
            &TransceiverConfig { quote, fail_quote, fail_send },
        );
    }

    pub fn quote_delivery_price(env: Env, _recipient_chain: u32) -> Result<i128, TransceiverError> {
        let cfg: TransceiverConfig = env.storage().instance().get(&symbol_short!("cfg")).unwrap();
        if cfg.fail_quote {
            return Err(TransceiverError::WormholeQueryFailed);
        }
        Ok(cfg.quote)
    }

    pub fn send_message(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    ) -> Result<(), TransceiverError> {
        let cfg: TransceiverConfig = env.storage().instance().get(&symbol_short!("cfg")).unwrap();
        if cfg.fail_send {
            return Err(TransceiverError::WormholePostFailed);
        }
        env.storage().instance().set(
            &symbol_short!("sent"),
            &SentMessage { recipient_chain, recipient_manager, manager_payload },
        );
        Ok(())
    }

    pub fn last_sent(env: Env) -> Option<SentMessage> {
        env.storage().instance().get(&symbol_short!("sent"))
    }
}

/// Deploys a mock transceiver and registers it on the manager. Requires the
/// owner's auth to be mocked (`set_transceiver` is owner-only).
pub fn add_transceiver(
    env: &Env,
    client: &ManagerContractClient,
    quote: i128,
    fail_quote: bool,
) -> Address {
    let transceiver = env.register(MockTransceiver, (quote, fail_quote, false));
    client.set_transceiver(&transceiver);
    transceiver
}
