use soroban_sdk::{contracttype, BytesN};

use crate::rate_limit::RateLimitParams;

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NttManagerPeer {
    pub address: BytesN<32>,
    pub token_decimals: u32,
    pub inbound_rate_limit: RateLimitParams,
}
