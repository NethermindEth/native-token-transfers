use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env};

use crate::errors::NttManagerError;
use crate::types::{
    AttestationInfo, AttestationResult, Mode, NttManagerPeer, TransferResult,
};

/// The core external API of an NTT Manager contract.
///
/// Defines the operations that external callers, peer transceivers, and
/// the client binding use to move tokens across chains: initiating outbound
/// transfers, processing inbound attestations, managing peers and
/// transceivers, and reading configuration.
///
/// The `#[contractclient]` attribute generates a `NttManagerClient` binding
/// that other contracts (notably transceivers) use to invoke this interface.
#[contractclient(name = "NttManagerClient")]
pub trait NttManagerInterface {
    /// Initiates an outbound transfer to a recipient on another chain.
    ///
    /// The `sender` must have authorized the call. If the amount exceeds
    /// the outbound rate limit, the behavior depends on `should_queue`:
    /// queued for later completion if `true`, rejected otherwise.
    fn transfer(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
    ) -> Result<TransferResult, NttManagerError>;
    /// Initiates an outbound transfer with an application-defined payload.
    ///
    /// Equivalent to [`transfer`](Self::transfer) but attaches
    /// `additional_payload` to the NTT message so the recipient manager can
    /// forward it to a downstream consumer.
    fn transfer_with_payload(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
        additional_payload: Bytes,
    ) -> Result<TransferResult, NttManagerError>;
    /// Completes an outbound transfer previously queued due to rate limiting.
    ///
    /// Only succeeds once the rate limit window has refilled enough capacity
    /// to release the queued amount.
    fn complete_queued_transfer(env: Env, sequence: u64)
        -> Result<TransferResult, NttManagerError>;
    /// Cancels an outbound transfer that is still queued.
    ///
    /// Only the original sender may cancel; the locked/burned tokens are
    /// returned to them.
    fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError>;
    /// Releases an inbound transfer previously queued due to rate limiting.
    ///
    /// The transfer is identified by its NTT message digest.
    fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError>;
    /// Records an attestation from a transceiver for an inbound message.
    ///
    /// Called by an enabled transceiver when it observes a message. When the
    /// attestation threshold is reached, the transfer is executed (or queued
    /// if the inbound rate limit is exceeded). The returned
    /// [`AttestationResult`] reports which stages completed.
    fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError>;
    /// Attempts to execute a message whose threshold has already been reached.
    ///
    /// Idempotent re-entry point for messages whose execution previously
    /// failed (e.g. token transfer reverted) or were queued due to inbound
    /// rate limiting.
    fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError>;
    /// Returns the decimal precision of the managed token as stored at init.
    fn token_decimals(env: Env) -> Result<u32, NttManagerError>;
    /// Returns the registered peer for `chain_id`, or `None` if unregistered.
    fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer>;
    /// Registers or updates a peer NTT Manager on another chain.
    ///
    /// Stores the peer address, its token decimals (used for amount
    /// normalization), and initializes its per-chain inbound rate limit.
    fn set_peer(
        env: Env,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError>;
    /// Updates the outbound rate limit capacity.
    ///
    /// Consumed capacity is adjusted proportionally so that pending queued
    /// transfers are not stranded by the change.
    fn set_outbound_limit(env: Env, limit: u64) -> Result<(), NttManagerError>;
    /// Updates the inbound rate limit capacity for a specific source chain.
    fn set_inbound_limit(env: Env, chain_id: u32, limit: u64) -> Result<(), NttManagerError>;
    /// Updates the attestation threshold, the number of distinct transceivers
    /// that must attest to a message before it executes.
    fn set_threshold(env: Env, threshold: u32) -> Result<(), NttManagerError>;
    /// Registers a new transceiver and returns the bitmap index assigned to it.
    ///
    /// Fails if the registry is full (64 entries) or the address is already
    /// registered.
    fn set_transceiver(env: Env, transceiver: Address) -> Result<u32, NttManagerError>;
    /// Disables a transceiver.
    ///
    /// The entry remains in the registry so its bitmap index can be reused
    /// later; only the enabled bit is cleared. Cannot disable the last
    /// enabled transceiver.
    fn remove_transceiver(env: Env, transceiver: Address) -> Result<(), NttManagerError>;
    /// Returns whether an inbound message with the given digest has already
    /// been executed.
    fn is_message_executed(env: Env, digest: BytesN<32>) -> bool;
    /// Returns the number of distinct attestations recorded for a message,
    /// counting only transceivers that are currently enabled. Disabling a
    /// transceiver retroactively removes its attestations from the count.
    fn message_attestations(env: Env, digest: BytesN<32>) -> u32;
    /// Returns whether a message has reached the attestation threshold.
    /// Always `false` when no threshold is configured.
    fn is_message_approved(env: Env, digest: BytesN<32>) -> bool;
    /// Returns whether the transceiver at `index` ever attested to the
    /// message. Reports raw historical state, unmasked by the enabled
    /// bitmap — a later-disabled transceiver still reads as having
    /// attested. `false` for `index >= MAX_TRANSCEIVERS`.
    fn transceiver_attested_to_message(env: Env, digest: BytesN<32>, index: u32) -> bool;
    /// Returns the next outbound transfer sequence number.
    fn get_next_sequence(env: Env) -> u64;
    /// Returns the token handling mode (locking or burning) set at init.
    fn get_mode(env: Env) -> Result<Mode, NttManagerError>;
    /// Returns the current attestation threshold.
    fn get_threshold(env: Env) -> u32;
    /// Returns the address of the managed token contract.
    fn get_token(env: Env) -> Result<Address, NttManagerError>;
    /// Returns the Wormhole chain ID that this manager believes it is on.
    fn get_chain_id(env: Env) -> Result<u32, NttManagerError>;
    /// Returns attestation tracking info for a message digest, if present.
    fn get_attestation_info(env: Env, digest: BytesN<32>) -> Option<AttestationInfo>;
    /// Returns the current contract version number.
    fn get_version(env: Env) -> u32;
}
