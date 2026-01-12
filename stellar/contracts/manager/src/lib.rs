#![no_std]

mod constants;
mod errors;
mod inbound;
mod messages;
mod outbound;
mod peers;
mod rate_limit;
mod state;
mod token_ops;
mod transceivers;

use errors::NttManagerError;
use inbound::{
    attestation_received_internal, complete_inbound_queued_transfer, execute_msg_internal,
};
use messages::TrimmedAmount;
use outbound::{
    cancel_outbound_queued_transfer, complete_outbound_queued_transfer, transfer_internal,
};
use peers::{
    set_inbound_limit as set_inbound_limit_internal, set_peer as set_peer_internal,
    NttManagerPeer,
};
use rate_limit::RateLimitParams;
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};
use state::{
    require_admin, require_not_paused, require_not_reentering, set_reentering, AttestationInfo,
    AttestationResult, DataKey, InboundQueuedTransfer, Mode, OutboundQueuedTransfer,
    TransferResult,
};
use token_ops::query_token_decimals;
use transceivers::{
    check_threshold_invariants, remove_transceiver as remove_transceiver_internal,
    set_threshold_value, set_transceiver as set_transceiver_internal, TransceiverInfo,
};

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

    /// Initiates a cross-chain token transfer.
    ///
    /// Transfers `amount` tokens from `sender` to `recipient` on `recipient_chain`.
    /// The transfer is validated, rate limited, and either sent immediately or queued
    /// depending on available capacity. If `should_queue` is false and the rate limit
    /// is exceeded, the transfer fails and tokens are returned.
    ///
    /// Returns a `TransferResult` with the sequence number, queue status, and message digest.
    ///
    /// # Errors
    /// - `ContractPaused` if the contract is paused
    /// - `Reentering` if a transfer is already in progress
    /// - `ZeroAmount` if amount is zero or negative
    /// - `InvalidRecipient` if recipient is all zeros
    /// - `PeerNotFound` if no peer registered for recipient chain
    /// - `TransferExceedsRateLimit` if rate limited and `should_queue` is false
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

    /// Initiates a cross-chain token transfer with custom payload.
    ///
    /// Same as `transfer` but includes `additional_payload` in the message.
    /// The payload can be used by the recipient for custom logic or data.
    ///
    /// # Errors
    /// Same error conditions as `transfer`.
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

    /// Completes a queued transfer after its release time.
    ///
    /// Can be called by anyone once the queued transfer's release timestamp is reached.
    /// Attempts to send the transfer if rate limit capacity is now available.
    ///
    /// # Errors
    /// - `ContractPaused` if the contract is paused
    /// - `Reentering` if another transfer is in progress
    /// - `TransferNotQueued` if no transfer exists for this sequence
    /// - `TransferNotReleasable` if release timestamp not yet reached
    /// - `TransferExceedsRateLimit` if still rate limited
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

    /// Cancels a queued transfer and refunds tokens.
    ///
    /// Only the original sender can cancel their queued transfer. Removes it
    /// from storage and returns the tokens to the sender.
    ///
    /// # Errors
    /// - `TransferNotQueued` if no transfer exists for this sequence
    /// - `CancellerNotSender` if caller is not the original sender
    pub fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError> {
        sender.require_auth();

        cancel_outbound_queued_transfer(&env, &sender, sequence)
    }

    /// Records an attestation from a transceiver for an inbound cross-chain message.
    ///
    /// Called by transceivers when they receive a verified message from another chain.
    /// Requires authentication from the calling transceiver. Once enough transceivers
    /// attest to meet the threshold, tokens are released to the recipient (or queued
    /// if the inbound rate limit is exceeded).
    ///
    /// Returns the attestation result indicating whether threshold was met, tokens
    /// were released, or the transfer was queued.
    pub fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        transceiver.require_auth();
        require_not_paused(&env)?;

        attestation_received_internal(
            &env,
            &transceiver,
            source_chain,
            &source_ntt_manager,
            &payload,
        )
    }

    /// Completes a rate-limited inbound transfer after its delay period.
    ///
    /// Permissionless: anyone can call once `release_timestamp` is reached.
    /// Releases the queued tokens to the original recipient and removes
    /// the transfer from the queue.
    pub fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError> {
        require_not_paused(&env)?;

        complete_inbound_queued_transfer(&env, &digest)
    }

    /// Manually executes an approved message that hasn't been executed yet.
    ///
    /// Permissionless recovery function for transfers where transceivers were
    /// disabled after attesting but before execution. Counts all attestations
    /// (even from now-disabled transceivers) when checking the threshold.
    ///
    /// Useful when the normal `attestation_received` path can't complete because
    /// attesting transceivers were subsequently disabled.
    pub fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        require_not_paused(&env)?;

        execute_msg_internal(&env, source_chain, &source_ntt_manager, &payload)
    }

    /// Returns the token address managed by this contract.
    ///
    /// # Panics
    /// Panics if the contract has not been initialized.
    pub fn get_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Token)
            .expect("not initialized")
    }

    /// Returns the operating mode (`Locking` or `Burning`).
    ///
    /// # Panics
    /// Panics if the contract has not been initialized.
    pub fn get_mode(env: Env) -> Mode {
        env.storage()
            .instance()
            .get(&DataKey::Mode)
            .expect("not initialized")
    }

    /// Returns this chain's Wormhole chain ID.
    ///
    /// # Panics
    /// Panics if the contract has not been initialized.
    pub fn get_chain_id(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::ChainId)
            .expect("not initialized")
    }

    /// Returns the current admin address.
    ///
    /// # Panics
    /// Panics if the contract has not been initialized.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized")
    }

    /// Returns the token's decimal precision (0-18).
    ///
    /// # Panics
    /// Panics if the contract has not been initialized.
    pub fn token_decimals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TokenDecimals)
            .expect("not initialized")
    }

    /// Returns the minimum number of transceiver attestations required
    /// to execute an inbound transfer. Returns 0 if no transceivers are registered.
    pub fn get_threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or(0)
    }

    /// Returns the total number of registered transceivers (enabled or disabled).
    pub fn get_transceiver_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TransceiverCount)
            .unwrap_or(0)
    }

    /// Returns a bitmap where bit N is set if transceiver index N is enabled.
    pub fn get_enabled_bitmap(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::EnabledBitmap)
            .unwrap_or(0)
    }

    /// Returns transceiver metadata by its permanent index.
    /// Returns `None` if no transceiver exists at the given index.
    pub fn get_transceiver_info(env: Env, index: u32) -> Option<TransceiverInfo> {
        env.storage().persistent().get(&DataKey::Transceiver(index))
    }

    /// Returns the peer NTT Manager configuration for a given chain.
    /// Returns `None` if no peer is registered for the chain ID.
    pub fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer> {
        env.storage().persistent().get(&DataKey::Peer(chain_id))
    }

    /// Returns the outbound rate limit parameters.
    /// If not initialized, returns unlimited capacity.
    pub fn get_outbound_limit_params(env: Env) -> RateLimitParams {
        env.storage()
            .instance()
            .get(&DataKey::OutboundRateLimit)
            .unwrap_or_else(|| RateLimitParams::new(u64::MAX, &env))
    }

    /// Returns the current outbound capacity, accounting for time-based refill.
    /// This is the maximum amount that can be transferred immediately without queueing.
    pub fn get_outbound_capacity(env: Env) -> u64 {
        let params: RateLimitParams = env
            .storage()
            .instance()
            .get(&DataKey::OutboundRateLimit)
            .unwrap_or_else(|| RateLimitParams::new(u64::MAX, &env));
        params.capacity_at(&env)
    }

    /// Returns the inbound rate limit parameters for a specific source chain.
    /// Returns `None` if no peer is registered for the chain ID.
    pub fn get_inbound_limit_params(env: Env, chain_id: u32) -> Option<RateLimitParams> {
        let peer: Option<NttManagerPeer> = env.storage().persistent().get(&DataKey::Peer(chain_id));
        peer.map(|p| p.inbound_rate_limit)
    }

    /// Returns the next outbound message sequence number.
    /// Sequence numbers start at 1 and increment with each transfer.
    pub fn get_next_sequence(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::NextSequence)
            .unwrap_or(1)
    }

    /// Checks whether tokens have been released for a given message digest.
    /// Returns `false` if the message has not been attested or executed.
    pub fn is_message_executed(env: Env, digest: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .get::<_, AttestationInfo>(&DataKey::Attestation(digest))
            .map(|a| a.executed)
            .unwrap_or(false)
    }

    /// Returns attestation tracking info for a message digest, including
    /// which transceivers have attested and whether execution occurred.
    pub fn get_attestation_info(env: Env, digest: BytesN<32>) -> Option<AttestationInfo> {
        env.storage()
            .persistent()
            .get(&DataKey::Attestation(digest))
    }

    /// Returns a queued outbound transfer by its sequence number.
    /// Returns `None` if no transfer is queued for this sequence.
    pub fn get_outbound_queue_item(env: Env, sequence: u64) -> Option<OutboundQueuedTransfer> {
        env.storage()
            .persistent()
            .get(&DataKey::OutboundQueue(sequence))
    }

    /// Returns a queued inbound transfer by its message digest.
    /// Returns `None` if no transfer is queued for this digest.
    pub fn get_inbound_queue_item(env: Env, digest: BytesN<32>) -> Option<InboundQueuedTransfer> {
        env.storage()
            .persistent()
            .get(&DataKey::InboundQueue(digest))
    }

    /// Computes the effective transfer amount after decimal normalization.
    ///
    /// Returns `(trimmed_amount, dust)` where `trimmed_amount` is what the
    /// recipient will receive and `dust` is the precision lost due to decimal
    /// differences between chains.
    ///
    /// # Errors
    /// - `PeerNotFound` if no peer is registered for `recipient_chain`
    /// - `NotInitialized` if token decimals are not set
    pub fn quote_transfer(
        env: Env,
        amount: i128,
        recipient_chain: u32,
    ) -> Result<(u64, u64), NttManagerError> {
        let peer: NttManagerPeer = env
            .storage()
            .persistent()
            .get(&DataKey::Peer(recipient_chain))
            .ok_or(NttManagerError::PeerNotFound)?;

        let our_decimals: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TokenDecimals)
            .ok_or(NttManagerError::NotInitialized)?;

        let (trimmed, dust) = TrimmedAmount::trim(
            amount as u128,
            our_decimals as u8,
            peer.token_decimals as u8,
        );

        Ok((trimmed.amount, dust as u64))
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Version)
            .unwrap_or(1)
    }

    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        Ok(())
    }

    pub fn validate_invariants(env: Env) -> Result<(), NttManagerError> {
        check_threshold_invariants(&env)
    }

    pub fn set_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<u32, NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        set_transceiver_internal(&env, transceiver)
    }

    pub fn remove_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        remove_transceiver_internal(&env, &transceiver)
    }

    pub fn set_threshold(
        env: Env,
        admin: Address,
        threshold: u32,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        set_threshold_value(&env, threshold)
    }

    pub fn set_peer(
        env: Env,
        admin: Address,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        set_peer_internal(&env, chain_id, peer_address, token_decimals, inbound_limit)
    }

    pub fn set_inbound_limit(
        env: Env,
        admin: Address,
        chain_id: u32,
        limit: u64,
    ) -> Result<(), NttManagerError> {
        extend_instance_ttl(&env);
        require_admin(&env, &admin)?;
        set_inbound_limit_internal(&env, chain_id, limit)
    }
}
