use soroban_ntt_client::TransceiverError;
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env};
use stellar_ntt_manager::ManagerContractClient;

#[contracttype]
struct TransceiverConfig {
    quote: i128,
    fail_quote: bool,
}

/// Minimal transceiver stand-in. The manager only reaches it through
/// `TransceiverClient`, so it implements just the methods the manager calls.
#[contract]
pub struct MockTransceiver;

#[contractimpl]
impl MockTransceiver {
    pub fn __constructor(env: Env, quote: i128, fail_quote: bool) {
        env.storage()
            .instance()
            .set(&symbol_short!("cfg"), &TransceiverConfig { quote, fail_quote });
    }

    pub fn quote_delivery_price(env: Env, _recipient_chain: u32) -> Result<i128, TransceiverError> {
        let cfg: TransceiverConfig = env.storage().instance().get(&symbol_short!("cfg")).unwrap();
        if cfg.fail_quote {
            return Err(TransceiverError::WormholeQueryFailed);
        }
        Ok(cfg.quote)
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
    let transceiver = env.register(MockTransceiver, (quote, fail_quote));
    client.set_transceiver(&transceiver);
    transceiver
}
