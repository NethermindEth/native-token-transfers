#![no_std]

mod constants;
mod errors;
mod messages;
mod peers;
mod outbound;
mod rate_limit;
mod state;
mod token_ops;
mod transceivers;

use errors::NttManagerError;
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};
use outbound::{
    cancel_outbound_queued_transfer, complete_outbound_queued_transfer, transfer_internal,
};
use state::{
    require_admin, require_not_paused, require_not_reentering, set_reentering, DataKey, Mode,
    TransferResult,
};
use token_ops::query_token_decimals;

use constants::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};


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
    /// - Rate limiting parameters (uses fixed 24-hour duration)
    /// - Initial sequence number and counters
    ///
    /// The token's decimal precision is queried and stored for amount normalization.
    pub fn __constructor(
        env: Env,
        admin: Address,
        token: Address,
        mode: Mode,
        chain_id: u32,
        outbound_limit: u64,
    ) {
        let token_decimals = query_token_decimals(&env, &token);

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
        env.storage().instance().set(&DataKey::EnabledBitmap, &0u64);

        let rate_limit_params = rate_limit::RateLimitParams::new(outbound_limit, &env);
        env.storage()
            .instance()
            .set(&DataKey::OutboundRateLimit, &rate_limit_params);

        extend_instance_ttl(&env);
    }

    pub fn receive_wormhole_message(
        _env: Env,
        _emitter_chain: u32,
        _emitter_address: BytesN<32>,
        _sequence: u64,
        _payload: Bytes,
    ) {
        // TODO
    }

    /// Initiates a two-step ownership transfer to a new admin.
    ///
    /// The current admin sets a pending admin, who must then call `accept_ownership`
    /// to complete the transfer. This prevents accidental transfers to invalid addresses.
    pub fn transfer_ownership(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &current_admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Completes a pending ownership transfer.
    ///
    /// Must be called by the address set as pending admin in `transfer_ownership`.
    /// Clears the pending admin after successful transfer.
    pub fn accept_ownership(env: Env, pending_admin: Address) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        pending_admin.require_auth();

        let stored_pending: Option<Address> = env.storage().instance().get(&DataKey::PendingAdmin);
        match stored_pending {
            Some(stored) if stored == pending_admin => {
                env.storage()
                    .instance()
                    .set(&DataKey::Admin, &pending_admin);
                env.storage().instance().remove(&DataKey::PendingAdmin);
                Ok(())
            }
            _ => Err(NttManagerError::InvalidPendingAdmin),
        }
    }

    /// Pauses the contract, blocking transfers and redemptions.
    ///
    /// Only callable by the admin. Use `unpause` to resume operations.
    pub fn pause(env: Env, admin: Address) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Unpauses the contract, resuming normal operations.
    ///
    /// Only callable by the admin.
    pub fn unpause(env: Env, admin: Address) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        state::is_paused(&env)
    }

    /// Updates the outbound rate limit.
    ///
    /// Adjusts the maximum transfer capacity proportionally. When reducing
    /// the limit, available capacity decreases by the difference.
    ///
    /// Only callable by the admin.
    pub fn set_outbound_limit(env: Env, admin: Address, limit: u64) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;

        let mut rate_limit_params = rate_limit::get_outbound_rate_limit(&env);
        rate_limit_params.set_limit(limit, &env);

        env.storage()
            .instance()
            .set(&DataKey::OutboundRateLimit, &rate_limit_params);

        Ok(())
    }

    pub fn transfer(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
    ) -> Result<TransferResult, NttManagerError> {
        sender.require_auth();
        require_not_paused(&env)?;
        require_not_reentering(&env)?;
        set_reentering(&env, true);

        let result = transfer_internal(
            &env,
            &sender,
            amount,
            recipient_chain,
            &recipient,
            should_queue,
            None,
        );

        set_reentering(&env, false);
        result
    }

    pub fn transfer_with_payload(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
        additional_payload: Bytes,
    ) -> Result<TransferResult, NttManagerError> {
        sender.require_auth();
        require_not_paused(&env)?;
        require_not_reentering(&env)?;
        set_reentering(&env, true);

        let result = transfer_internal(
            &env,
            &sender,
            amount,
            recipient_chain,
            &recipient,
            should_queue,
            Some(additional_payload),
        );

        set_reentering(&env, false);
        result
    }

    pub fn complete_queued_transfer(
        env: Env,
        sequence: u64,
    ) -> Result<TransferResult, NttManagerError> {
        require_not_paused(&env)?;
        require_not_reentering(&env)?;
        set_reentering(&env, true);

        let result = complete_outbound_queued_transfer(&env, sequence);

        set_reentering(&env, false);
        result
    }

    pub fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError> {
        sender.require_auth();

        cancel_outbound_queued_transfer(&env, &sender, sequence)
    }
}

