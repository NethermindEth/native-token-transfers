use soroban_sdk::{contractclient, contracterror, contracttype, Address, Bytes, BytesN, Env};

use crate::rate_limit::RateLimitParams;

/// Token handling mode for the NTT Manager.
///
/// Determines how the manager handles tokens during cross-chain transfers:
/// - `Locking`: Tokens are locked in the contract (used on the canonical chain)
/// - `Burning`: Tokens are burned/minted (used on non-canonical chains)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
#[repr(u32)]
pub enum Mode {
    /// Lock tokens in the contract. Used when this chain holds the canonical token.
    Locking = 0,
    /// Burn tokens on send, mint on receive. Used for wrapped/synthetic tokens.
    Burning = 1,
}

/// Peer NTT Manager on another chain.
///
/// Each peer maintains its own inbound rate limit, allowing independent
/// throttling of transfers from different source chains.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NttManagerPeer {
    /// 32-byte address of the NTT Manager on the peer chain.
    pub address: BytesN<32>,
    /// Token decimals on the peer chain (1-18). Used for amount normalization.
    pub token_decimals: u32,
    /// Rate limiter for inbound transfers from this chain.
    pub inbound_rate_limit: RateLimitParams,
}

/// Result of a transfer operation, returned to the caller.
///
/// Contains the sequence number for tracking, whether the transfer was queued
/// due to rate limiting, and the message digest for verification.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransferResult {
    /// Unique sequence number assigned to this transfer.
    pub sequence: u64,
    /// Whether this transfer was queued (`true`) or sent immediately (`false`).
    pub queued: bool,
    /// Keccak-256 digest of the NTT message payload.
    pub digest: BytesN<32>,
}

/// Result of processing an attestation from a transceiver.
///
/// Indicates whether the attestation threshold was met, whether tokens
/// were released, and whether the transfer was queued due to rate limiting.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct AttestationResult {
    /// Whether the attestation threshold is now met.
    pub approved: bool,
    /// Whether tokens were released to the recipient.
    pub executed: bool,
    /// Whether the transfer was queued due to rate limiting.
    pub queued: bool,
}

/// Errors returned by the NTT Manager contract.
///
/// Variants are grouped by numeric range so clients can classify failures
/// without string matching:
/// - `1..=7`: message encoding, decoding, or amount normalization
/// - `10..=13`: authorization and pause state
/// - `20`, `30`: uninitialized storage or components
/// - `40..=48`: transceiver registry and threshold management
/// - `50..=55`: peer registration and validation
/// - `60..=65`: outbound/inbound transfer flow
/// - `80..=84`: attestation processing and redemption
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum NttManagerError {
    /// Payload is shorter than the minimum NTT message length.
    MessageTooShort = 1,
    /// NTT message prefix does not match the expected magic bytes.
    InvalidPrefix = 2,
    /// Decimal value is outside the supported range (must be 1..=18).
    InvalidDecimals = 3,
    /// Chain identifier exceeds `u16::MAX`, which Wormhole chain IDs must fit in.
    ChainIdTooLarge = 4,
    /// Additional transfer payload exceeds the maximum encodable length.
    PayloadTooLong = 5,
    /// Source and destination token decimals cannot be reconciled without loss.
    DecimalMismatch = 6,
    /// Arithmetic overflow while normalizing or summing a transfer amount.
    AmountOverflow = 7,
    /// Caller is not the admin for a privileged operation.
    Unauthorized = 10,
    /// The provided pending admin does not match the stored pending admin.
    InvalidPendingAdmin = 11,
    /// Operation rejected because the contract is currently paused.
    ContractPaused = 12,
    /// Caller is neither the admin nor the designated pauser.
    NotAdminOrPauser = 13,
    /// Rate limit parameters have not been initialized for this context.
    RateLimitNotInitialized = 20,
    /// Required contract state has not been initialized.
    NotInitialized = 30,
    /// The address provided is not a registered transceiver.
    TransceiverNotRegistered = 40,
    /// The transceiver registry is full (bitmap capacity, 64 entries).
    MaxTransceiversReached = 41,
    /// Attestation threshold is zero while one or more transceivers are registered.
    ZeroThreshold = 42,
    /// Attestation threshold exceeds the number of enabled transceivers.
    ThresholdTooHigh = 43,
    /// The transceiver is already enabled.
    TransceiverAlreadyEnabled = 44,
    /// The transceiver is already disabled.
    TransceiverAlreadyDisabled = 45,
    /// Cannot disable the only remaining enabled transceiver.
    CannotDisableLastTransceiver = 46,
    /// There are no enabled transceivers to send through.
    NoEnabledTransceivers = 47,
    /// Transceiver bitmap index is outside the representable range.
    BitmapIndexOutOfRange = 48,
    /// No peer is registered for the given chain ID.
    PeerNotFound = 50,
    /// Peer chain ID cannot be zero.
    InvalidPeerChainIdZero = 51,
    /// Peer chain ID equals this manager's own chain ID.
    InvalidPeerSameChainId = 52,
    /// Peer address is the zero address.
    InvalidPeerZeroAddress = 53,
    /// Peer token decimals are outside the valid range.
    InvalidPeerDecimals = 54,
    /// Peer parameters failed generic validation.
    InvalidPeer = 55,
    /// Transfer amount is zero after normalization.
    ZeroAmount = 60,
    /// Recipient address is invalid (e.g. zero address).
    InvalidRecipient = 61,
    /// Transfer would exceed the configured rate limit and `should_queue` was false.
    TransferExceedsRateLimit = 62,
    /// No outbound transfer is queued for the given sequence.
    TransferNotQueued = 63,
    /// Queued inbound transfer is not yet eligible for release.
    TransferNotReleasable = 64,
    /// The caller is not the original sender of the queued outbound transfer.
    CancellerNotSender = 65,
    /// Attestation received from a transceiver that is not enabled.
    TransceiverNotEnabled = 80,
    /// This transceiver has already attested to the given message.
    TransceiverAlreadyAttested = 81,
    /// The inbound transfer has already been redeemed.
    TransferAlreadyRedeemed = 82,
    /// Message target chain does not match this manager's chain ID.
    InvalidTargetChain = 83,
    /// Transfer has not reached the attestation threshold yet.
    TransferNotApproved = 84,
}

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
        admin: Address,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError>;
    /// Updates the outbound rate limit capacity.
    ///
    /// Consumed capacity is adjusted proportionally so that pending queued
    /// transfers are not stranded by the change.
    fn set_outbound_limit(env: Env, admin: Address, limit: u64) -> Result<(), NttManagerError>;
    /// Updates the inbound rate limit capacity for a specific source chain.
    fn set_inbound_limit(
        env: Env,
        admin: Address,
        chain_id: u32,
        limit: u64,
    ) -> Result<(), NttManagerError>;
    /// Updates the attestation threshold, the number of distinct transceivers
    /// that must attest to a message before it executes.
    fn set_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), NttManagerError>;
    /// Registers a new transceiver and returns the bitmap index assigned to it.
    ///
    /// Fails if the registry is full (64 entries) or the address is already
    /// registered.
    fn set_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<u32, NttManagerError>;
    /// Disables a transceiver.
    ///
    /// The entry remains in the registry so its bitmap index can be reused
    /// later; only the enabled bit is cleared. Cannot disable the last
    /// enabled transceiver.
    fn remove_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<(), NttManagerError>;
    /// Returns whether an inbound message with the given digest has already
    /// been executed.
    fn is_message_executed(env: Env, digest: BytesN<32>) -> bool;
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
}
