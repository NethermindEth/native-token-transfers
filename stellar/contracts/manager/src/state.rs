use soroban_sdk::{address_payload::AddressPayload, contracttype, Address, Bytes, BytesN, Env};

use crate::{errors::NttManagerError, messages::TrimmedAmount};
use crate::constants::{PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD};

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
    /// SHA-256 digest of the NTT message payload.
    pub digest: BytesN<32>,
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct AttestationInfo {
    pub executed: bool,
    pub attested_transceivers: u64,
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct InboundQueuedTransfer {
    pub recipient: Address,
    pub amount: i128,
    pub release_timestamp: u64,
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct AttestationResult {
    pub approved: bool,
    pub executed: bool,
    pub queued: bool,
}

/// Retrieves the current admin address.
///
/// # Panics
/// Panics if the contract has not been initialized (admin not set).
pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("admin not set")
}

/// Verifies the caller is the current admin.
///
/// Requires authentication from `caller` and checks they match the stored admin.
/// Returns `Unauthorized` if the caller is not the admin.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), NttManagerError> {
    caller.require_auth();
    let admin = get_admin(env);
    if *caller != admin {
        return Err(NttManagerError::Unauthorized);
    }
    Ok(())
}

/// Returns whether the contract is currently paused.
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Ensures the contract is not paused.
///
/// Returns `ContractPaused` if the contract is currently paused.
pub fn require_not_paused(env: &Env) -> Result<(), NttManagerError> {
    if is_paused(env) {
        return Err(NttManagerError::ContractPaused);
    }
    Ok(())
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

/// Atomically increments and returns the next message sequence number.
///
/// Sequence numbers start at 1 and are used to uniquely identify outbound
/// transfers. The counter is incremented before returning, so each call
/// gets a unique, monotonically increasing value.
pub fn use_message_sequence(env: &Env) -> u64 {
    let current: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextSequence)
        .unwrap_or(1);
    env.storage()
        .instance()
        .set(&DataKey::NextSequence, &(current + 1));
    current
}

/// Derives a deterministic message ID from a sequence number.
///
/// Computes SHA-256 hash of the big-endian encoded sequence number.
/// Used to uniquely identify NTT messages across chains.
pub fn sequence_to_message_id(env: &Env, sequence: u64) -> BytesN<32> {
    let mut data = soroban_sdk::Bytes::new(env);
    data.append(&soroban_sdk::Bytes::from_array(
        env,
        &sequence.to_be_bytes(),
    ));
    env.crypto().sha256(&data).into()
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
