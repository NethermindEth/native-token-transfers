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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum NttManagerError {
    MessageTooShort = 1,
    InvalidPrefix = 2,
    InvalidDecimals = 3,
    ChainIdTooLarge = 4,
    PayloadTooLong = 5,
    DecimalMismatch = 6,
    AmountOverflow = 7,
    Unauthorized = 10,
    InvalidPendingAdmin = 11,
    ContractPaused = 12,
    NotAdminOrPauser = 13,
    RateLimitNotInitialized = 20,
    NotInitialized = 30,
    TransceiverNotRegistered = 40,
    MaxTransceiversReached = 41,
    ZeroThreshold = 42,
    ThresholdTooHigh = 43,
    TransceiverAlreadyEnabled = 44,
    TransceiverAlreadyDisabled = 45,
    CannotDisableLastTransceiver = 46,
    NoEnabledTransceivers = 47,
    BitmapIndexOutOfRange = 48,
    PeerNotFound = 50,
    InvalidPeerChainIdZero = 51,
    InvalidPeerSameChainId = 52,
    InvalidPeerZeroAddress = 53,
    InvalidPeerDecimals = 54,
    InvalidPeer = 55,
    ZeroAmount = 60,
    InvalidRecipient = 61,
    TransferExceedsRateLimit = 62,
    TransferNotQueued = 63,
    TransferNotReleasable = 64,
    CancellerNotSender = 65,
    TransceiverNotEnabled = 80,
    TransceiverAlreadyAttested = 81,
    TransferAlreadyRedeemed = 82,
    InvalidTargetChain = 83,
    TransferNotApproved = 84,
}

#[contractclient(name = "NttManagerClient")]
pub trait NttManagerInterface {
    fn transfer(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
    ) -> Result<TransferResult, NttManagerError>;
    fn transfer_with_payload(
        env: Env,
        sender: Address,
        amount: i128,
        recipient_chain: u32,
        recipient: BytesN<32>,
        should_queue: bool,
        additional_payload: Bytes,
    ) -> Result<TransferResult, NttManagerError>;
    fn complete_queued_transfer(env: Env, sequence: u64)
        -> Result<TransferResult, NttManagerError>;
    fn cancel_queued_transfer(
        env: Env,
        sender: Address,
        sequence: u64,
    ) -> Result<(), NttManagerError>;
    fn complete_inbound_transfer(env: Env, digest: BytesN<32>) -> Result<(), NttManagerError>;
    fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError>;
    fn execute_msg(
        env: Env,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError>;
    fn token_decimals(env: Env) -> Result<u32, NttManagerError>;
    fn get_peer(env: Env, chain_id: u32) -> Option<NttManagerPeer>;
    fn set_peer(
        env: Env,
        admin: Address,
        chain_id: u32,
        peer_address: BytesN<32>,
        token_decimals: u32,
        inbound_limit: u64,
    ) -> Result<(), NttManagerError>;
    fn set_outbound_limit(env: Env, admin: Address, limit: u64) -> Result<(), NttManagerError>;
    fn set_inbound_limit(
        env: Env,
        admin: Address,
        chain_id: u32,
        limit: u64,
    ) -> Result<(), NttManagerError>;
    fn set_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), NttManagerError>;
    fn set_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<u32, NttManagerError>;
    fn remove_transceiver(
        env: Env,
        admin: Address,
        transceiver: Address,
    ) -> Result<(), NttManagerError>;
    fn is_message_executed(env: Env, digest: BytesN<32>) -> bool;
    fn get_next_sequence(env: Env) -> u64;
    fn get_mode(env: Env) -> Result<Mode, NttManagerError>;
    fn get_threshold(env: Env) -> u32;
    fn get_token(env: Env) -> Result<Address, NttManagerError>;
    fn get_chain_id(env: Env) -> Result<u32, NttManagerError>;
}
