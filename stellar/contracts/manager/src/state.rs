use soroban_sdk::{address_payload::AddressPayload, contracttype, Address, Bytes, BytesN, Env};

use crate::constants::{PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD};
use crate::rate_limit::RateLimitParams;
use crate::{errors::NttManagerError, messages::TrimmedAmount};

pub fn extend_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
}

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

impl Mode {
    pub fn is_locking(&self) -> bool {
        matches!(self, Mode::Locking)
    }

    pub fn is_burning(&self) -> bool {
        matches!(self, Mode::Burning)
    }
}

/// Storage keys for contract state.
///
/// Keys are organized by storage type:
/// - Simple keys: Stored in instance storage (config, counters)
/// - Parameterized keys: Stored in persistent storage (peers, attestations, queues)
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // Instance storage - core configuration
    Admin,
    PendingAdmin,
    /// Separate pauser role for emergency pause operations.
    Pauser,
    Token,
    TokenDecimals,
    Mode,
    ChainId,
    Paused,
    /// Minimum number of transceiver attestations required to execute a transfer.
    Threshold,
    /// Monotonically increasing sequence number for outbound messages.
    NextSequence,
    Version,
    TransceiverCount,
    /// Bitmap of enabled transceivers (bit N = transceiver index N is enabled).
    EnabledBitmap,
    OutboundRateLimit,
    /// Rate limit refill duration in seconds.
    RateLimitDuration,

    // Persistent storage - per-chain/message data
    /// Peer NTT Manager address for a given chain ID.
    Peer(u32),
    /// Transceiver contract address by index.
    Transceiver(u32),
    /// Reverse lookup: transceiver address to index.
    TransceiverIndex(Address),
    /// Attestation bitmap for a message digest.
    Attestation(BytesN<32>),
    /// Queued outbound transfer by sequence number.
    OutboundQueue(u64),
    /// Queued inbound transfer by message digest.
    InboundQueue(BytesN<32>),

    /// Reentrancy guard flag.
    Reentering,
}

/// Aggregated configuration for the NTT Manager.
///
/// Returned by `get_config()` to provide a snapshot of current settings.
#[derive(Clone, Debug)]
#[contracttype]
pub struct NttConfig {
    pub mode: Mode,
    pub token: Address,
    pub token_decimals: u32,
    pub chain_id: u32,
    pub admin: Address,
    pub paused: bool,
    pub threshold: u32,
}

/// A transfer that exceeded the rate limit and was queued for later completion.
///
/// Stored in persistent storage keyed by sequence number. Anyone can complete
/// the transfer once `release_timestamp` is reached. Only the original sender
/// can cancel it to reclaim their tokens.
#[derive(Clone, Debug)]
#[contracttype]
pub struct OutboundQueuedTransfer {
    /// Original sender who initiated the transfer.
    pub sender: Address,
    /// Normalized amount with dust removed.
    pub amount: TrimmedAmount,
    /// Destination Wormhole chain ID.
    pub recipient_chain: u32,
    /// NTT Manager address on the destination chain.
    pub recipient_ntt_manager: BytesN<32>,
    /// Final recipient address on the destination chain.
    pub recipient: BytesN<32>,
    /// Token contract address (converted to bytes32).
    pub source_token: BytesN<32>,
    /// Ledger timestamp when the transfer becomes eligible for completion.
    pub release_timestamp: u64,
    /// Optional custom payload attached to the transfer.
    pub additional_payload: Option<Bytes>,
}

/// Result of a transfer operation, returned to the caller.
///
/// Contains the sequence number for tracking, whether the transfer was queued
/// due to rate limiting, and the message digest for verification.
#[derive(Clone, Debug)]
#[contracttype]
pub struct TransferResult {
    /// Unique sequence number assigned to this transfer.
    pub sequence: u64,
    /// Whether this transfer was queued (`true`) or sent immediately (`false`).
    pub queued: bool,
    /// Keccak-256 digest of the NTT message payload.
    pub digest: BytesN<32>,
}

/// Tracks attestation state for an inbound cross-chain message.
///
/// Stored in persistent storage keyed by message digest. Used for replay
/// protection and to track which transceivers have attested to the message.
#[derive(Clone, Debug)]
#[contracttype]
pub struct AttestationInfo {
    /// Whether tokens have been released for this message.
    pub executed: bool,
    /// Bitmap of transceiver indices that have attested (bit N = transceiver N attested).
    pub attested_transceivers: u64,
}

/// An inbound transfer that exceeded the rate limit and was queued.
///
/// Stored in persistent storage keyed by message digest. Anyone can complete
/// the transfer after `release_timestamp` is reached.
#[derive(Clone, Debug)]
#[contracttype]
pub struct InboundQueuedTransfer {
    /// Recipient address on this chain.
    pub recipient: Address,
    /// Amount in local token decimals (already untrimmed).
    pub amount: i128,
    /// Original trimmed amount from the wire format, used for rate limit backflow.
    pub trimmed_amount: u64,
    /// Ledger timestamp when the transfer becomes eligible for completion.
    pub release_timestamp: u64,
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

/// Result of processing an attestation from a transceiver.
///
/// Indicates whether the attestation threshold was met, whether tokens
/// were released, and whether the transfer was queued due to rate limiting.
#[derive(Clone, Debug)]
#[contracttype]
pub struct AttestationResult {
    /// Whether the attestation threshold is now met.
    pub approved: bool,
    /// Whether tokens were released to the recipient.
    pub executed: bool,
    /// Whether the transfer was queued due to rate limiting.
    pub queued: bool,
}

/// Ensures no reentrant call is in progress.
///
/// Checks temporary storage for the reentrancy guard flag. Returns `Reentering`
/// error if a transfer operation is already executing in the call stack.
pub fn require_not_reentering(env: &Env) -> Result<(), NttManagerError> {
    let reentering: bool = env
        .storage()
        .temporary()
        .get(&DataKey::Reentering)
        .unwrap_or(false);
    if reentering {
        return Err(NttManagerError::Reentering);
    }
    Ok(())
}

/// Sets the reentrancy guard flag in temporary storage.
///
/// Used to prevent reentrant calls during token operations. The flag is automatically
/// cleared at the end of the transaction since it uses temporary storage.
pub fn set_reentering(env: &Env, reentering: bool) {
    env.storage()
        .temporary()
        .set(&DataKey::Reentering, &reentering);
}

/// Converts a sequence number to a 32-byte message ID.
///
/// Encodes the sequence as big-endian u64 in the last 8 bytes (right-aligned),
/// with the first 24 bytes as zeros.
pub fn sequence_to_message_id(env: &Env, sequence: u64) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&sequence.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

/// Converts a Soroban `Address` to a 32-byte representation.
///
/// Extracts the underlying bytes from either an account ID (Ed25519 public key)
/// or contract ID hash. Used for cross-chain address encoding in NTT messages.
///
/// # Panics
/// Panics if the address has no payload (should never happen with valid addresses).
pub fn address_to_bytes32(_env: &Env, address: &Address) -> BytesN<32> {
    match address.to_payload().expect("address has no payload") {
        AddressPayload::AccountIdPublicKeyEd25519(bytes) => bytes,
        AddressPayload::ContractIdHash(bytes) => bytes,
    }
}

/// Reconstructs a Soroban `Address` from 32 bytes.
///
/// Assumes the bytes represent an account ID (Ed25519 public key).
/// Used when decoding recipient addresses from cross-chain messages.
pub fn bytes32_to_address(env: &Env, bytes: &BytesN<32>) -> Address {
    Address::from_payload(
        env,
        AddressPayload::AccountIdPublicKeyEd25519(bytes.clone()),
    )
}
