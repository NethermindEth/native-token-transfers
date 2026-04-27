pub use soroban_ntt_client::{
    AttestationInfo, AttestationResult, InboundQueuedTransfer, Mode, NttManagerPeer,
    OutboundQueuedTransfer, TransferResult,
};
use soroban_sdk::{address_payload::AddressPayload, contracttype, Address, BytesN, Env};

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
