#![no_std]

mod constants;
mod errors;
mod inbound;
mod messages;
mod outbound;
mod peers;
mod rate_limit;
mod state;
mod storage;
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
use peers::{set_inbound_limit as set_inbound_limit_internal, set_peer as set_peer_internal};
use rate_limit::RateLimitParams;
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};
use state::{
    require_not_reentering, set_reentering, AttestationInfo, AttestationResult, DataKey,
    InboundQueuedTransfer, Mode, NttManagerPeer, OutboundQueuedTransfer, TransferResult,
};
use storage::{
    AttestationEntry, InboundQueueEntry, InstanceStorage, OutboundQueueEntry, PeerEntry,
    TransceiverEntry,
};
use token_ops::query_token_decimals;
use transceivers::{
    check_threshold_invariants, remove_transceiver as remove_transceiver_internal,
    set_threshold_value, set_transceiver as set_transceiver_internal, TransceiverInfo,
};

/// Executes a state-modifying operation with pause and reentrancy guards.
///
/// Checks that the contract is not paused, prevents reentrant calls, and ensures
/// the reentrancy flag is cleared after execution (even on error).
fn with_transfer_guard<F, T>(env: &Env, f: F) -> Result<T, NttManagerError>
where
    F: FnOnce() -> Result<T, NttManagerError>,
{
    let storage = InstanceStorage::new(env);
    storage.require_not_paused()?;
    require_not_reentering(env)?;
    set_reentering(env, true);

    let result = f();

    set_reentering(env, false);
    result
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
    /// - Rate limiting parameters with configurable duration
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
        rate_limit_duration: u64,
    ) {
        let token_decimals = query_token_decimals(&env, &token);

        // Use direct storage access for initialization (InstanceStorage extends TTL)
        let storage = env.storage().instance();
        storage.set(&DataKey::Admin, &admin);
        storage.set(&DataKey::Token, &token);
        storage.set(&DataKey::TokenDecimals, &token_decimals);
        storage.set(&DataKey::Mode, &mode);
        storage.set(&DataKey::ChainId, &chain_id);
        storage.set(&DataKey::Paused, &false);
        storage.set(&DataKey::Threshold, &0u32);
        storage.set(&DataKey::NextSequence, &1u64);
        storage.set(&DataKey::Version, &1u32);
        storage.set(&DataKey::TransceiverCount, &0u32);
        storage.set(&DataKey::EnabledBitmap, &0u64);
        storage.set(&DataKey::RateLimitDuration, &rate_limit_duration);

        let rate_limit_params = rate_limit::RateLimitParams::new(outbound_limit, &env);
        storage.set(&DataKey::OutboundRateLimit, &rate_limit_params);

        // Extend TTL after initialization
        let _ = InstanceStorage::new(&env);
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
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&current_admin)?;
        storage.set_pending_admin(&new_admin);
        Ok(())
    }

    /// Completes a pending ownership transfer.
    ///
    /// Must be called by the address set as pending admin in `transfer_ownership`.
    /// Clears the pending admin after successful transfer.
    pub fn accept_ownership(env: Env, pending_admin: Address) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        pending_admin.require_auth();

        let stored = storage
            .pending_admin()
            .ok_or(NttManagerError::InvalidPendingAdmin)?;

        if stored != pending_admin {
            return Err(NttManagerError::InvalidPendingAdmin);
        }

        storage.set_admin(&pending_admin);
        storage.remove_pending_admin();
        Ok(())
    }

    /// Pauses the contract, blocking transfers and redemptions.
    ///
    /// Only callable by the admin. Use `unpause` to resume operations.
    pub fn pause(env: Env, caller: Address) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin_or_pauser(&caller)?;
        storage.set_paused(true);
        Ok(())
    }

    /// Unpauses the contract, resuming normal operations.
    ///
    /// Callable by either the admin or the designated pauser (if set).
    pub fn unpause(env: Env, caller: Address) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin_or_pauser(&caller)?;
        storage.set_paused(false);
        Ok(())
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        InstanceStorage::new(&env).is_paused()
    }

    /// Transfers the pauser capability to a new address.
    ///
    /// Callable by either the admin or the current pauser (if set). Pass `None`
    /// to remove the pauser role entirely, restricting pause operations to admin only.
    pub fn transfer_pauser(
        env: Env,
        caller: Address,
        new_pauser: Option<Address>,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin_or_pauser(&caller)?;
        storage.set_pauser(new_pauser.as_ref());
        Ok(())
    }

    /// Updates the outbound rate limit.
    ///
    /// Adjusts the maximum transfer capacity proportionally. When reducing
    /// the limit, available capacity decreases by the difference.
    ///
    /// Only callable by the admin.
    pub fn set_outbound_limit(env: Env, admin: Address, limit: u64) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;

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
        with_transfer_guard(&env, || {
            transfer_internal(
                &env,
                &sender,
                amount,
                recipient_chain,
                &recipient,
                should_queue,
                None,
            )
        })
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
        with_transfer_guard(&env, || {
            transfer_internal(
                &env,
                &sender,
                amount,
                recipient_chain,
                &recipient,
                should_queue,
                Some(additional_payload.clone()),
            )
        })
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
        with_transfer_guard(&env, || complete_outbound_queued_transfer(&env, sequence))
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
        let _ = InstanceStorage::new(&env);
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
        with_transfer_guard(&env, || {
            attestation_received_internal(
                &env,
                &transceiver,
                source_chain,
                &source_ntt_manager,
                &payload,
            )
        })
    }

    /// Completes a rate-limited inbound transfer after its delay period.
    ///
    /// Permissionless: anyone can call once `release_timestamp` is reached.
    /// Releases the queued tokens to the original recipient and removes
    /// the transfer from the queue.
    pub fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError> {
        with_transfer_guard(&env, || complete_inbound_queued_transfer(&env, &digest))
    }

    /// Manually executes an approved message that hasn't been executed yet.
    ///
    /// Permissionless recovery function for transfers where transceivers were
    /// disabled after attesting but before execution. Only attestations from
    /// currently enabled transceivers count toward the threshold.
    ///
    /// Useful when the normal `attestation_received` path can't complete due
    /// to rate limiting or transaction failure.
    pub fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        with_transfer_guard(&env, || {
            execute_msg_internal(&env, source_chain, &source_ntt_manager, &payload)
        })
    }

    /// Returns the token address managed by this contract.
    pub fn get_token(env: Env) -> Result<Address, NttManagerError> {
        InstanceStorage::new(&env).token()
    }

    /// Returns the operating mode (`Locking` or `Burning`).
    pub fn get_mode(env: Env) -> Result<Mode, NttManagerError> {
        InstanceStorage::new(&env).mode()
    }

    /// Returns this chain's Wormhole chain ID.
    pub fn get_chain_id(env: Env) -> Result<u32, NttManagerError> {
        InstanceStorage::new(&env).chain_id()
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Result<Address, NttManagerError> {
        InstanceStorage::new(&env).admin()
    }

    /// Returns the designated pauser address, if one has been set.
    ///
    /// When set, this address can pause/unpause the contract independently
    /// of the admin. Returns `None` if no pauser has been configured.
    pub fn get_pauser(env: Env) -> Option<Address> {
        InstanceStorage::new(&env).pauser()
    }

    /// Returns the token's decimal precision (0-18).
    pub fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        InstanceStorage::new(&env).token_decimals()
    }

    /// Returns the minimum number of transceiver attestations required
    /// to execute an inbound transfer. Returns 0 if no transceivers are registered.
    pub fn get_threshold(env: Env) -> u32 {
        let storage = InstanceStorage::new(&env);
        storage.threshold()
    }

    /// Returns the total number of registered transceivers (enabled or disabled).
    pub fn get_transceiver_count(env: Env) -> u32 {
        let storage = InstanceStorage::new(&env);
        storage.transceiver_count()
    }

    /// Returns a bitmap where bit N is set if transceiver index N is enabled.
    pub fn get_enabled_bitmap(env: Env) -> u64 {
        let storage = InstanceStorage::new(&env);
        storage.enabled_bitmap()
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
        PeerEntry::new(&env, chain_id)
            .get()
            .map(|p| p.inbound_rate_limit)
    }

    /// Returns the next outbound message sequence number.
    /// Sequence numbers start at 1 and increment with each transfer.
    pub fn get_next_sequence(env: Env) -> u64 {
        let storage = InstanceStorage::new(&env);
        storage.next_sequence()
    }

    /// Checks whether tokens have been released for a given message digest.
    /// Returns `false` if the message has not been attested or executed.
    pub fn is_message_executed(env: Env, digest: BytesN<32>) -> bool {
        AttestationEntry::new(&env, digest)
            .get()
            .map(|a| a.executed)
            .unwrap_or(false)
    }

    /// Returns attestation tracking info for a message digest, including
    /// which transceivers have attested and whether execution occurred.
    pub fn get_attestation_info(env: Env, digest: BytesN<32>) -> Option<AttestationInfo> {
        AttestationEntry::new(&env, digest).get()
    }

    /// Returns a queued outbound transfer by its sequence number.
    /// Returns `None` if no transfer is queued for this sequence.
    pub fn get_outbound_queue_item(env: Env, sequence: u64) -> Option<OutboundQueuedTransfer> {
        OutboundQueueEntry::new(&env, sequence).get()
    }

    /// Returns a queued inbound transfer by its message digest.
    /// Returns `None` if no transfer is queued for this digest.
    pub fn get_inbound_queue_item(env: Env, digest: BytesN<32>) -> Option<InboundQueuedTransfer> {
        InboundQueueEntry::new(&env, digest).get()
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
        let storage = InstanceStorage::new(&env);

        let peer = PeerEntry::new(&env, recipient_chain).get_or_err()?;

        let our_decimals = storage.token_decimals()?;

        let (trimmed, dust) = TrimmedAmount::trim(
            amount as u128,
            our_decimals as u8,
            peer.token_decimals as u8,
        );

        Ok((trimmed.amount, dust as u64))
    }

    /// Returns the current contract version number.
    ///
    /// Defaults to 1 if no version has been explicitly set.
    pub fn get_version(env: Env) -> u32 {
        let storage = InstanceStorage::new(&env);
        storage.version()
    }

    /// Returns the rate limit duration in seconds.
    ///
    /// Defaults to 86400 (24 hours) if not explicitly set.
    pub fn get_rate_limit_duration(env: Env) -> u64 {
        let storage = InstanceStorage::new(&env);
        storage.rate_limit_duration()
    }

    /// Upgrades the contract to a new WASM implementation.
    ///
    /// The new WASM must be previously installed on the network. After upgrade,
    /// the contract uses the new code for all subsequent invocations while
    /// preserving existing state.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        Ok(())
    }

    /// Validates critical contract invariants.
    ///
    /// Permissionless check that verifies:
    /// - INV-023: `threshold <= enabled_transceiver_count`
    /// - INV-024: `threshold > 0` when transceivers exist
    ///
    /// # Errors
    /// - `ThresholdTooHigh` if threshold exceeds enabled count
    /// - `ZeroThreshold` if transceivers exist but threshold is 0
    pub fn validate_invariants(env: Env) -> Result<(), NttManagerError> {
        check_threshold_invariants(&env)
    }

    /// Registers a new transceiver or re-enables a disabled one.
    ///
    /// Transceivers receive permanent indices (0-63) that persist even if disabled.
    /// The first registered transceiver automatically sets threshold to 1.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    /// - `MaxTransceiversReached` if 64 transceivers already registered
    /// - `TransceiverAlreadyEnabled` if transceiver is already active
    pub fn set_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<u32, NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_transceiver_internal(&env, transceiver)
    }

    /// Disables a transceiver, excluding it from attestation voting.
    ///
    /// The transceiver remains registered but won't count toward thresholds.
    /// Threshold is automatically reduced if it would exceed enabled count.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    /// - `TransceiverNotRegistered` if address not registered
    /// - `TransceiverAlreadyDisabled` if already disabled
    /// - `CannotDisableLastTransceiver` if this is the only enabled transceiver
    pub fn remove_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        remove_transceiver_internal(&env, &transceiver)
    }

    /// Sets the minimum attestation threshold for inbound transfers.
    ///
    /// The threshold must be at least 1 and cannot exceed the number of
    /// enabled transceivers.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    /// - `ZeroThreshold` if threshold is 0
    /// - `ThresholdTooHigh` if threshold exceeds enabled count
    pub fn set_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_threshold_value(&env, threshold)
    }

    /// Registers or updates a peer NTT Manager on another chain.
    ///
    /// Each peer has its own inbound rate limit. When updating an existing
    /// peer, the rate limit capacity is adjusted proportionally.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    /// - `InvalidPeerChainIdZero` if chain_id is 0
    /// - `InvalidPeerSameChainId` if chain_id matches this chain
    /// - `InvalidPeerZeroAddress` if address is all zeros
    /// - `InvalidPeerDecimals` if decimals is 0 or > 18
    pub fn set_peer(
        env: Env,
        admin: Address,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_peer_internal(&env, chain_id, peer_address, token_decimals, inbound_limit)
    }

    /// Updates the inbound rate limit for a specific peer chain.
    ///
    /// Adjusts capacity proportionally when changing the limit.
    ///
    /// # Errors
    /// - `Unauthorized` if caller is not the admin
    /// - `PeerNotFound` if no peer registered for chain_id
    pub fn set_inbound_limit(
        env: Env,
        admin: Address,
        chain_id: u32,
        limit: u64,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_inbound_limit_internal(&env, chain_id, limit)
    }
}
