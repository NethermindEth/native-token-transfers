#![no_std]

mod constants;
mod inbound;
mod messages;
mod outbound;
mod peers;
mod rate_limit;
mod state;
mod storage;
mod token_ops;
mod transceivers;

use inbound::{
    attestation_received_internal, complete_inbound_queued_transfer, execute_msg_internal,
};
use messages::TrimmedAmount;
use outbound::{
    cancel_outbound_queued_transfer, complete_outbound_queued_transfer, transfer_internal,
};
use peers::{set_inbound_limit as set_inbound_limit_internal, set_peer as set_peer_internal};
use soroban_ntt_client::{NttManagerError, NttManagerInterface, NttManagerPeer, RateLimitParams};
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};
pub use state::AttestationResult;
use state::{AttestationInfo, InboundQueuedTransfer, Mode, OutboundQueuedTransfer, TransferResult};
use storage::{
    AttestationEntry, InboundQueueEntry, InstanceStorage, OutboundQueueEntry, PeerEntry,
    TransceiverEntry,
};
use token_ops::query_token_decimals;
use transceivers::{
    check_threshold_invariants, remove_transceiver as remove_transceiver_internal,
    set_threshold_value, set_transceiver as set_transceiver_internal, TransceiverInfo,
};

/// Executes a state-modifying operation with a pause guard.
///
/// Checks that the contract is not paused before executing the closure.
fn with_transfer_guard<F, T>(env: &Env, f: F) -> Result<T, NttManagerError>
where
    F: FnOnce() -> Result<T, NttManagerError>,
{
    InstanceStorage::new(env).require_not_paused()?;
    f()
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
        let storage = InstanceStorage::new(&env);

        // TODO: Implement as validation function in core contract interface
        // Can we panic here?
        if chain_id > u16::MAX as u32 {
            panic!("chain_id exceeds u16::MAX");
        }

        storage.set_admin(&admin);
        storage.set_token(&token);
        storage.set_token_decimals(token_decimals);
        storage.set_mode(&mode);
        storage.set_chain_id(chain_id);
        storage.set_paused(false);
        storage.set_threshold(0);
        storage.set_next_sequence(1);
        storage.set_version(1);
        storage.set_transceiver_count(0);
        storage.set_enabled_bitmap(0);
        storage.set_rate_limit_duration(rate_limit_duration);
        storage.set_outbound_rate_limit(&RateLimitParams::new(outbound_limit, &env));
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

    /// Returns the total number of registered transceivers (enabled or disabled).
    pub fn get_transceiver_count(env: Env) -> u32 {
        InstanceStorage::new(&env).transceiver_count()
    }

    /// Returns a bitmap where bit N is set if transceiver index N is enabled.
    pub fn get_enabled_bitmap(env: Env) -> u64 {
        InstanceStorage::new(&env).enabled_bitmap()
    }

    /// Returns transceiver metadata by its permanent index.
    /// Returns `None` if no transceiver exists at the given index.
    pub fn get_transceiver_info(env: Env, index: u32) -> Option<TransceiverInfo> {
        TransceiverEntry::new(&env, index).get()
    }

    /// Returns the outbound rate limit parameters.
    /// If not initialized, returns unlimited capacity.
    pub fn get_outbound_limit_params(env: Env) -> RateLimitParams {
        InstanceStorage::new(&env).outbound_rate_limit()
    }

    /// Returns the current outbound capacity, accounting for time-based refill.
    /// This is the maximum amount that can be transferred immediately without queueing.
    pub fn get_outbound_capacity(env: Env) -> u64 {
        let storage = InstanceStorage::new(&env);
        let rate_limit = storage.outbound_rate_limit();
        rate_limit.capacity_at(&env, storage.rate_limit_duration())
    }

    /// Returns the inbound rate limit parameters for a specific source chain.
    /// Returns `None` if no peer is registered for the chain ID.
    pub fn get_inbound_limit_params(env: Env, chain_id: u32) -> Option<RateLimitParams> {
        PeerEntry::new(&env, chain_id)
            .get()
            .map(|p| p.inbound_rate_limit)
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
        )?;

        Ok((trimmed.amount, dust as u64))
    }

    /// Returns the current contract version number.
    ///
    /// Defaults to 1 if no version has been explicitly set.
    pub fn get_version(env: Env) -> u32 {
        InstanceStorage::new(&env).version()
    }

    /// Returns the rate limit duration in seconds.
    ///
    /// Defaults to 86400 (24 hours) if not explicitly set.
    pub fn get_rate_limit_duration(env: Env) -> u64 {
        InstanceStorage::new(&env).rate_limit_duration()
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
}

#[contractimpl]
impl NttManagerInterface for ManagerContract {
    fn transfer(
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

    fn transfer_with_payload(
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

    fn complete_queued_transfer(
        env: Env,
        sequence: u64,
    ) -> Result<TransferResult, NttManagerError> {
        with_transfer_guard(&env, || complete_outbound_queued_transfer(&env, sequence))
    }

    fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError> {
        sender.require_auth();
        cancel_outbound_queued_transfer(&env, &sender, sequence)
    }

    fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError> {
        with_transfer_guard(&env, || complete_inbound_queued_transfer(&env, &digest))
    }

    fn attestation_received(
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

    fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        with_transfer_guard(&env, || {
            execute_msg_internal(&env, source_chain, &source_ntt_manager, &payload)
        })
    }

    fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        InstanceStorage::new(&env).token_decimals()
    }

    fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer> {
        PeerEntry::new(&env, chain_id).get()
    }

    fn set_peer(
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

    fn set_outbound_limit(env: Env, admin: Address, limit: u64) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;

        let mut rate_limit_params = storage.outbound_rate_limit();
        rate_limit_params.set_limit(limit, &env, storage.rate_limit_duration());
        storage.set_outbound_rate_limit(&rate_limit_params);

        Ok(())
    }

    fn set_inbound_limit(
        env: Env,
        admin: Address,
        chain_id: u32,
        limit: u64,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_inbound_limit_internal(&env, chain_id, limit)
    }

    fn set_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_threshold_value(&env, threshold)
    }

    fn set_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<u32, NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        set_transceiver_internal(&env, transceiver)
    }

    fn remove_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        storage.require_admin(&admin)?;
        remove_transceiver_internal(&env, &transceiver)
    }

    fn is_message_executed(env: Env, digest: BytesN<32>) -> bool {
        AttestationEntry::new(&env, digest)
            .get()
            .map(|a| a.executed)
            .unwrap_or(false)
    }

    fn get_next_sequence(env: Env) -> u64 {
        InstanceStorage::new(&env).next_sequence()
    }

    fn get_mode(env: Env) -> Result<Mode, NttManagerError> {
        InstanceStorage::new(&env).mode()
    }

    fn get_threshold(env: Env) -> u32 {
        InstanceStorage::new(&env).threshold()
    }

    fn get_token(env: Env) -> Result<Address, NttManagerError> {
        InstanceStorage::new(&env).token()
    }

    fn get_chain_id(env: Env) -> Result<u32, NttManagerError> {
        InstanceStorage::new(&env).chain_id()
    }
}
