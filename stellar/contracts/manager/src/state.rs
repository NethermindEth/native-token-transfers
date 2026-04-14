pub use soroban_ntt_client::{AttestationResult, Mode, NttManagerPeer, TransferResult};
use soroban_sdk::{address_payload::AddressPayload, contracttype, Address, Bytes, BytesN, Env};

use crate::messages::TrimmedAmount;

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
pub fn address_to_bytes32(address: &Address) -> BytesN<32> {
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
