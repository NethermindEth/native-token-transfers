use soroban_sdk::{contracttype, Address, BytesN, Env};

use crate::errors::NttManagerError;

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
    RateLimitDuration,
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
    pub rate_limit_duration: u64,
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
