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
    /// Decimals value exceeds the maximum allowed (8).
    InvalidDecimals = 3,
    /// Chain ID exceeds the maximum allowed (65535).
    ChainIdTooLarge = 4,
    /// Payload length exceeds maximum (65535 bytes).
    PayloadTooLong = 5,
    /// TrimmedAmount arithmetic on operands with different decimals.
    DecimalMismatch = 6,

    /// Caller is not authorized to perform the requested action.
    Unauthorized = 10,
    /// No pending admin set, or caller does not match the pending admin address.
    InvalidPendingAdmin = 11,

    /// Contract is paused; transfers and redemptions are blocked.
    ContractPaused = 12,
    /// Caller is neither the admin nor the pauser.
    NotAdminOrPauser = 13,

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
    /// Transceiver is already enabled.
    TransceiverAlreadyEnabled = 44,
    /// Transceiver is already disabled.
    TransceiverAlreadyDisabled = 45,
    /// Cannot disable the last enabled transceiver.
    CannotDisableLastTransceiver = 46,
    /// No transceivers are enabled; transfers cannot proceed.
    NoEnabledTransceivers = 47,

    /// No peer registered for the specified chain ID.
    PeerNotFound = 50,
    /// Peer chain ID cannot be zero.
    InvalidPeerChainIdZero = 51,
    /// Peer chain ID cannot match this contract's chain ID.
    InvalidPeerSameChainId = 52,
    /// Peer address cannot be all zeros.
    InvalidPeerZeroAddress = 53,
    /// Peer token decimals must be between 1 and 18 inclusive.
    InvalidPeerDecimals = 54,
    /// Message source address does not match the registered peer.
    InvalidPeer = 55,

    /// Transfer amount is zero or negative.
    ZeroAmount = 60,
    /// Recipient address is all zeros.
    InvalidRecipient = 61,
    /// Transfer exceeds rate limit and should_queue is false.
    TransferExceedsRateLimit = 62,
    /// Queued transfer not found for the given sequence number.
    TransferNotQueued = 63,
    /// Queued transfer cannot be completed yet (release timestamp not reached).
    TransferNotReleasable = 64,
    /// Caller is not the original sender of the queued transfer.
    CancellerNotSender = 65,

    /// Function is re-entering (reentrancy guard triggered).
    Reentering = 70,

    /// Transceiver exists but is currently disabled; cannot attest.
    TransceiverNotEnabled = 80,
    /// This transceiver has already attested to this message digest.
    TransceiverAlreadyAttested = 81,
    /// Tokens have already been released for this message; replay blocked.
    TransferAlreadyRedeemed = 82,
    /// Message destination chain does not match this contract's chain ID.
    InvalidTargetChain = 83,
    /// Attestation threshold not met, or attestation record does not exist.
    TransferNotApproved = 84,
}
