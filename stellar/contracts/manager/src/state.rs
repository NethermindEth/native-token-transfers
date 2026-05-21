pub use soroban_ntt_client::{
    AttestationInfo, AttestationResult, InboundQueuedTransfer, Mode, NttManagerPeer,
    OutboundQueuedTransfer, TransferResult,
};
use soroban_sdk::{contracttype, Address, BytesN};

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
