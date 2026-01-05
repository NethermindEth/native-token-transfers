use soroban_sdk::contracterror;

/// Contract errors for the NTT Manager.
///
/// Error codes are grouped by category:
/// - 1-9: Message parsing errors
/// - 10-19: Authorization errors
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum NttManagerError {
    /// Input bytes are shorter than the minimum required length for the message type.
    MessageTooShort = 1,
    /// Message prefix does not match the expected magic bytes (`0x994E5454`).
    InvalidPrefix = 2,

    /// Caller is not authorized to perform the requested action.
    Unauthorized = 10,
    /// No pending admin set, or caller does not match the pending admin address.
    InvalidPendingAdmin = 11,

    /// Contract is paused; transfers and redemptions are blocked.
    ContractPaused = 12,

    /// Rate limit not initialized.
    RateLimitNotInitialized = 20,

    /// Contract not initialized (missing required configuration).
    NotInitialized = 30,
    /// Transceiver address is not registered in the registry.
    TransceiverNotRegistered = 40,
    /// Cannot register more than 64 transceivers.
    MaxTransceiversReached = 41,
    /// Threshold cannot be zero when transceivers are registered.
    ZeroThreshold = 42,
    /// Threshold exceeds the number of enabled transceivers.
    ThresholdTooHigh = 43,
}
