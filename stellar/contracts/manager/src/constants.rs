//! Constants used throughout the NTT Manager contract.

/// TTL threshold in ledgers (~1 day at 5s/ledger) before extending.
pub const INSTANCE_TTL_THRESHOLD: u32 = 17280;
/// TTL extension in ledgers (~30 days at 5s/ledger).
pub const INSTANCE_TTL_EXTEND: u32 = 17280 * 30;
/// TTL threshold for persistent storage (~1 day at 5s/ledger).
pub const PERSISTENT_TTL_THRESHOLD: u32 = 17280;
/// TTL extension for persistent storage (~30 days at 5s/ledger).
pub const PERSISTENT_TTL_EXTEND: u32 = 17280 * 30;

/// Rate limit refill duration in seconds (24 hours).
///
/// The bucket fully refills over this period.
pub const RATE_LIMIT_DURATION: u64 = 86400;

/// Maximum number of transceivers that can be registered.
///
/// Limited to 64 because transceiver state is tracked via a `u64` bitmap.
pub const MAX_TRANSCEIVERS: u32 = 64;