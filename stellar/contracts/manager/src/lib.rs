#![no_std]

mod messages;
mod state;
mod errors;

use errors::NttManagerError;
use soroban_sdk::{contract, contractimpl, token, Address, Bytes, BytesN, Env};
use state::{require_admin, DataKey, Mode};

/// TTL threshold in ledgers (~1 day at 5s/ledger) before extending.
const INSTANCE_TTL_THRESHOLD: u32 = 17280;
/// TTL extension in ledgers (~30 days at 5s/ledger).
const INSTANCE_TTL_EXTEND: u32 = 17280 * 30;

/// Queries the decimal precision of a token contract.
fn get_token_decimals(env: &Env, token: &Address) -> u32 {
    let client = token::Client::new(env, token);
    client.decimals()
}

/// Extends the instance storage TTL to prevent expiration.
fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
}

/// NTT Manager contract for cross-chain native token transfers.
///
/// Coordinates token locking/burning, message sequencing, and transceiver
/// attestation for secure cross-chain transfers via the Wormhole protocol.
#[contract]
pub struct ManagerContract;

#[contractimpl]
impl ManagerContract {
    /// Initializes the NTT Manager with the given configuration.
    ///
    /// Called automatically at contract deployment (Protocol 22+). Sets up:
    /// - Admin and token configuration
    /// - Operating mode (locking or burning)
    /// - Rate limiting parameters
    /// - Initial sequence number and counters
    ///
    /// The token's decimal precision is queried and stored for amount normalization.
    pub fn __constructor(
        env: Env,
        admin: Address,
        token: Address,
        mode: Mode,
        chain_id: u32,
        rate_limit_duration: u64,
        outbound_limit: u64,
    ) {
        let token_decimals = get_token_decimals(&env, &token);

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage()
            .instance()
            .set(&DataKey::TokenDecimals, &token_decimals);
        env.storage().instance().set(&DataKey::Mode, &mode);
        env.storage().instance().set(&DataKey::ChainId, &chain_id);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::Threshold, &0u32);
        env.storage().instance().set(&DataKey::NextSequence, &1u64);
        env.storage().instance().set(&DataKey::Version, &1u32);
        env.storage()
            .instance()
            .set(&DataKey::TransceiverCount, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::EnabledBitmap, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::RateLimitDuration, &rate_limit_duration);
        env.storage()
            .instance()
            .set(&DataKey::OutboundRateLimit, &outbound_limit);

        extend_instance_ttl(&env);
    }

    // Called by Transceiver after wormhole verification + peer transceiver check
    pub fn receive_wormhole_message(
        _env: Env,
        _emitter_chain: u32,
        _emitter_address: BytesN<32>,
        _sequence: u64,
        _payload: Bytes,
    ) {
        // TODO
    }
}
