#![no_std]

mod address_registry;
mod inbound;
mod outbound;
mod peers;
mod rate_limit;
mod state;
mod storage;
mod token_ops;
mod transceivers;

use inbound::{
    attestation_received_internal, complete_inbound_queued_transfer, count_valid_attestations,
    execute_msg_internal,
};
use outbound::{
    cancel_outbound_queued_transfer, complete_outbound_queued_transfer, transfer_internal,
};
use peers::{set_inbound_limit as set_inbound_limit_internal, set_peer as set_peer_internal};
use soroban_ntt_client::{
    validate_chain_id, AttestationInfo, InboundQueuedTransfer, NttManagerError,
    NttManagerInterface, NttManagerPeer, OutboundQueuedTransfer, RateLimitParams,
    RateLimiterInterface, TransceiverClient, TransceiverFee, TrimmedAmount,
};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Bytes, BytesN, Env, Vec};
pub use state::AttestationResult;
use state::{Mode, TransferResult};
use stellar_access::ownable::{self, Ownable};
use stellar_contract_utils::pausable::{self, Pausable};
use stellar_macros::{only_owner, when_not_paused};
use storage::{
    AttestationEntry, InboundQueueEntry, InstanceStorage, OutboundQueueEntry, PeerEntry,
    TransceiverEntry,
};
use token_ops::query_token_decimals;
use transceivers::{
    check_threshold_invariants, get_enabled_transceivers,
    remove_transceiver as remove_transceiver_internal, set_threshold_value,
    set_transceiver as set_transceiver_internal, Bitmap, TransceiverInfo,
};

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
    /// Sets up:
    /// - Owner and token configuration
    /// - Operating mode (locking or burning)
    /// - Rate limiting parameters with configurable duration
    /// - Initial sequence number and counters
    ///
    /// The token's decimal precision is queried and stored for amount normalization.
    pub fn __constructor(
        env: Env,
        owner: Address,
        token: Address,
        mode: Mode,
        chain_id: u32,
        outbound_limit: u64,
        rate_limit_duration: u64,
    ) {
        let token_decimals = query_token_decimals(&env, &token);
        let storage = InstanceStorage::new(&env);

        if validate_chain_id(chain_id).is_none() {
            panic_with_error!(&env, NttManagerError::ChainIdTooLarge);
        }

        ownable::set_owner(&env, &owner);
        storage.set_token(&token);
        storage.set_token_decimals(token_decimals);
        storage.set_mode(&mode);
        storage.set_chain_id(chain_id);
        storage.set_threshold(0);
        storage.set_next_sequence(1);
        storage.set_version(1);
        storage.set_transceiver_count(0);
        storage.set_enabled_bitmap(0);
        storage.set_rate_limit_duration(rate_limit_duration);
        storage.set_outbound_rate_limit(&RateLimitParams::new(outbound_limit, &env));
    }

    /// Transfers the pauser capability to a new address.
    ///
    /// Callable by either the contract owner or the current pauser (if set).
    /// Pass `None` to remove the pauser role entirely, leaving `pause` as an
    /// owner-only capability.
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

    /// Returns the designated pauser address, if one has been set.
    ///
    /// When set, this address can pause the contract (emergency stop)
    /// independently of the owner. Only the owner can unpause. Returns `None`
    /// if no pauser has been configured.
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

    /// Computes the effective transfer amount after decimal normalization.
    ///
    /// Returns `(trimmed_amount, dust)` where `trimmed_amount` is what the
    /// recipient receives and `dust` is the precision lost to decimal
    /// differences between the local and peer token. Delivery fees are quoted
    /// separately via [`quote_delivery_price`](Self::quote_delivery_price).
    ///
    /// # Errors
    /// - `PeerNotFound` if no peer is registered for `recipient_chain`
    /// - `NotInitialized` if token decimals are not set
    pub fn quote_transfer(
        env: Env,
        amount: i128,
        recipient_chain: u32,
    ) -> Result<(u64, u64), NttManagerError> {
        validate_chain_id(recipient_chain).ok_or(NttManagerError::ChainIdTooLarge)?;

        if amount <= 0 {
            return Err(NttManagerError::ZeroAmount);
        }

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

    /// Quotes the delivery fee for each enabled transceiver.
    ///
    /// Returns one [`TransceiverFee`] per enabled transceiver, in registry
    /// order. A transceiver whose quote call fails yields `fee: None` rather
    /// than failing the whole query, so the caller can see which transceiver
    /// is unavailable. Sum the `Some` values for the total dispatch cost.
    ///
    /// Makes one cross-contract call per enabled transceiver, so cost scales
    /// linearly with the registry size; intended as an off-chain query rather
    /// than a hot on-chain path.
    ///
    /// # Errors
    /// - `ChainIdTooLarge` if `recipient_chain` exceeds the Wormhole range
    pub fn quote_delivery_price(
        env: Env,
        recipient_chain: u32,
    ) -> Result<Vec<TransceiverFee>, NttManagerError> {
        validate_chain_id(recipient_chain).ok_or(NttManagerError::ChainIdTooLarge)?;

        let mut fees = Vec::new(&env);
        for transceiver in get_enabled_transceivers(&env)?.iter() {
            let fee = match TransceiverClient::new(&env, &transceiver)
                .try_quote_delivery_price(&recipient_chain)
            {
                Ok(Ok(fee)) => Some(fee),
                _ => None,
            };
            fees.push_back(TransceiverFee { transceiver, fee });
        }
        Ok(fees)
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
    /// preserving existing state. Restricted to the contract owner.
    #[only_owner]
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), NttManagerError> {
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

    /// Records a Soroban address under its canonical `hash_address` and returns
    /// the hash. Permissionless and idempotent — the key is derived from the
    /// value, so the mapping cannot be poisoned. A recipient must call this
    /// before an inbound transfer addressed to it can be redeemed.
    pub fn record_address(env: Env, address: Address) -> BytesN<32> {
        address_registry::record_address(&env, &address)
    }

    /// Resolves a recorded address hash, or `RecipientNotRegistered` if unknown.
    pub fn get_address_from_hash(
        env: Env,
        hash: BytesN<32>,
    ) -> Result<Address, NttManagerError> {
        address_registry::resolve_address(&env, &hash)
    }
}

#[contractimpl(contracttrait)]
impl Ownable for ManagerContract {}

#[contractimpl(contracttrait)]
impl Pausable for ManagerContract {
    // Owner-or-pauser can pause (emergency stop); only the owner can unpause.
    // Matches EVM `ManagerBase`: a compromised pauser can DoS but cannot resume.
    fn pause(env: &Env, caller: Address) {
        if let Err(e) = InstanceStorage::new(env).require_admin_or_pauser(&caller) {
            panic_with_error!(env, e);
        }
        pausable::pause(env);
    }

    fn unpause(env: &Env, _caller: Address) {
        ownable::enforce_owner_auth(env);
        pausable::unpause(env);
    }
}

#[contractimpl]
impl NttManagerInterface for ManagerContract {
    #[when_not_paused]
    fn transfer(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
    ) -> Result<TransferResult, NttManagerError> {
        sender.require_auth();
        validate_chain_id(recipient_chain).ok_or(NttManagerError::ChainIdTooLarge)?;
        transfer_internal(
            &env,
            &sender,
            amount,
            recipient_chain,
            &recipient,
            should_queue,
            None,
        )
    }

    #[when_not_paused]
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
        validate_chain_id(recipient_chain).ok_or(NttManagerError::ChainIdTooLarge)?;
        transfer_internal(
            &env,
            &sender,
            amount,
            recipient_chain,
            &recipient,
            should_queue,
            Some(additional_payload),
        )
    }

    #[when_not_paused]
    fn complete_queued_transfer(
        env: Env,
        sequence: u64,
    ) -> Result<TransferResult, NttManagerError> {
        complete_outbound_queued_transfer(&env, sequence)
    }

    fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError> {
        sender.require_auth();
        cancel_outbound_queued_transfer(&env, &sender, sequence)
    }

    #[when_not_paused]
    fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError> {
        complete_inbound_queued_transfer(&env, &digest)
    }

    #[when_not_paused]
    fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        transceiver.require_auth();
        validate_chain_id(source_chain).ok_or(NttManagerError::ChainIdTooLarge)?;
        attestation_received_internal(
            &env,
            &transceiver,
            source_chain,
            &source_ntt_manager,
            &payload,
        )
    }

    #[when_not_paused]
    fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        validate_chain_id(source_chain).ok_or(NttManagerError::ChainIdTooLarge)?;
        execute_msg_internal(&env, source_chain, &source_ntt_manager, &payload)
    }

    fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        InstanceStorage::new(&env).token_decimals()
    }

    fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer> {
        PeerEntry::new(&env, chain_id).get()
    }

    #[only_owner]
    fn set_peer(
        env: Env,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError> {
        validate_chain_id(chain_id).ok_or(NttManagerError::ChainIdTooLarge)?;
        set_peer_internal(&env, chain_id, peer_address, token_decimals, inbound_limit)
    }

    #[only_owner]
    fn set_outbound_limit(env: Env, limit: u64) -> Result<(), NttManagerError> {
        let storage = InstanceStorage::new(&env);
        let mut rate_limit_params = storage.outbound_rate_limit();
        rate_limit_params.set_limit(limit, &env, storage.rate_limit_duration());
        storage.set_outbound_rate_limit(&rate_limit_params);
        Ok(())
    }

    #[only_owner]
    fn set_inbound_limit(env: Env, chain_id: u32, limit: u64) -> Result<(), NttManagerError> {
        validate_chain_id(chain_id).ok_or(NttManagerError::ChainIdTooLarge)?;
        set_inbound_limit_internal(&env, chain_id, limit)
    }

    #[only_owner]
    fn set_threshold(env: Env, threshold: u32) -> Result<(), NttManagerError> {
        set_threshold_value(&env, threshold)
    }

    #[only_owner]
    fn set_transceiver(env: Env, transceiver: Address) -> Result<u32, NttManagerError> {
        set_transceiver_internal(&env, transceiver)
    }

    #[only_owner]
    fn remove_transceiver(env: Env, transceiver: Address) -> Result<(), NttManagerError> {
        remove_transceiver_internal(&env, &transceiver)
    }

    fn is_message_executed(env: Env, digest: BytesN<32>) -> bool {
        AttestationEntry::new(&env, digest)
            .get()
            .map(|a| a.executed)
            .unwrap_or(false)
    }

    fn message_attestations(env: Env, digest: BytesN<32>) -> u32 {
        AttestationEntry::new(&env, digest)
            .get()
            .map(|a| count_valid_attestations(&env, &a).0)
            .unwrap_or(0)
    }

    fn is_message_approved(env: Env, digest: BytesN<32>) -> bool {
        AttestationEntry::new(&env, digest).get().is_some_and(|a| {
            let (count, threshold) = count_valid_attestations(&env, &a);
            threshold > 0 && count >= threshold
        })
    }

    fn transceiver_attested_to_message(env: Env, digest: BytesN<32>, index: u32) -> bool {
        AttestationEntry::new(&env, digest)
            .get()
            .is_some_and(|a| Bitmap(a.attested_transceivers).is_set(index).unwrap_or(false))
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

    fn get_attestation_info(env: Env, digest: BytesN<32>) -> Option<AttestationInfo> {
        AttestationEntry::new(&env, digest).get()
    }

    fn get_version(env: Env) -> u32 {
        InstanceStorage::new(&env).version()
    }
}

#[contractimpl]
impl RateLimiterInterface for ManagerContract {
    fn get_outbound_limit_params(env: Env) -> RateLimitParams {
        InstanceStorage::new(&env).outbound_rate_limit()
    }

    fn get_outbound_capacity(env: Env) -> u64 {
        let storage = InstanceStorage::new(&env);
        let rate_limit = storage.outbound_rate_limit();
        rate_limit.capacity_at(&env, storage.rate_limit_duration())
    }

    fn get_inbound_capacity(env: Env, chain_id: u32) -> Option<u64> {
        let storage = InstanceStorage::new(&env);
        let duration = storage.rate_limit_duration();

        PeerEntry::new(&env, chain_id)
            .get()
            .map(|peer| peer.inbound_rate_limit.capacity_at(&env, duration))
    }

    fn get_inbound_limit_params(env: Env, chain_id: u32) -> Option<RateLimitParams> {
        PeerEntry::new(&env, chain_id)
            .get()
            .map(|peer| peer.inbound_rate_limit)
    }

    fn get_outbound_queue_item(env: Env, sequence: u64) -> Option<OutboundQueuedTransfer> {
        OutboundQueueEntry::new(&env, sequence).get()
    }

    fn get_inbound_queue_item(env: Env, digest: BytesN<32>) -> Option<InboundQueuedTransfer> {
        InboundQueueEntry::new(&env, digest).get()
    }
}
