//! Contract error vocabularies returned across the NTT manager and transceiver.
//!
//! Discriminants are part of the on-chain ABI: appending variants is safe,
//! reordering or reusing retired numbers is not.

use soroban_sdk::contracterror;

/// Errors returned by the NTT Manager contract.
///
/// Variants are grouped by numeric range so clients can classify failures
/// without string matching:
/// - `1..=7`: message encoding, decoding, or amount normalization
/// - `10..=13`: authorization and pause state
/// - `20`, `30`: uninitialized storage or components
/// - `40..=48`: transceiver registry and threshold management
/// - `50..=55`: peer registration and validation
/// - `60..=65`: outbound/inbound transfer flow
/// - `80..=84`: attestation processing and redemption
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum NttManagerError {
    /// Payload is shorter than the minimum NTT message length.
    MessageTooShort = 1,
    /// NTT message prefix does not match the expected magic bytes.
    InvalidPrefix = 2,
    /// Decimal value is outside the supported range (must be 1..=18).
    InvalidDecimals = 3,
    /// Chain identifier exceeds `u16::MAX`, which Wormhole chain IDs must fit in.
    ChainIdTooLarge = 4,
    /// Additional transfer payload exceeds the maximum encodable length.
    PayloadTooLong = 5,
    /// Source and destination token decimals cannot be reconciled without loss.
    DecimalMismatch = 6,
    /// Arithmetic overflow while normalizing or summing a transfer amount.
    AmountOverflow = 7,
    /// Caller is not the admin for a privileged operation.
    Unauthorized = 10,
    /// The provided pending admin does not match the stored pending admin.
    InvalidPendingAdmin = 11,
    /// Operation rejected because the contract is currently paused.
    ContractPaused = 12,
    /// Caller is neither the admin nor the designated pauser.
    NotAdminOrPauser = 13,
    /// Rate limit parameters have not been initialized for this context.
    RateLimitNotInitialized = 20,
    /// Required contract state has not been initialized.
    NotInitialized = 30,
    /// The address provided is not a registered transceiver.
    TransceiverNotRegistered = 40,
    /// The transceiver registry is full (bitmap capacity, 64 entries).
    MaxTransceiversReached = 41,
    /// Attestation threshold is zero while one or more transceivers are registered.
    ZeroThreshold = 42,
    /// Attestation threshold exceeds the number of enabled transceivers.
    ThresholdTooHigh = 43,
    /// The transceiver is already enabled.
    TransceiverAlreadyEnabled = 44,
    /// The transceiver is already disabled.
    TransceiverAlreadyDisabled = 45,
    /// Cannot disable the only remaining enabled transceiver.
    CannotDisableLastTransceiver = 46,
    /// There are no enabled transceivers to send through.
    NoEnabledTransceivers = 47,
    /// Transceiver bitmap index is outside the representable range.
    BitmapIndexOutOfRange = 48,
    /// No peer is registered for the given chain ID.
    PeerNotFound = 50,
    /// Peer chain ID cannot be zero.
    InvalidPeerChainIdZero = 51,
    /// Peer chain ID equals this manager's own chain ID.
    InvalidPeerSameChainId = 52,
    /// Peer address is the zero address.
    InvalidPeerZeroAddress = 53,
    /// Peer token decimals are outside the valid range.
    InvalidPeerDecimals = 54,
    /// Peer parameters failed generic validation.
    InvalidPeer = 55,
    /// Transfer amount is zero after normalization.
    ZeroAmount = 60,
    /// Recipient address is invalid (e.g. zero address).
    InvalidRecipient = 61,
    /// Transfer would exceed the configured rate limit and `should_queue` was false.
    TransferExceedsRateLimit = 62,
    /// No outbound transfer is queued for the given sequence.
    TransferNotQueued = 63,
    /// Queued inbound transfer is not yet eligible for release.
    TransferNotReleasable = 64,
    /// The caller is not the original sender of the queued outbound transfer.
    CancellerNotSender = 65,
    /// Attestation received from a transceiver that is not enabled.
    TransceiverNotEnabled = 80,
    /// This transceiver has already attested to the given message.
    TransceiverAlreadyAttested = 81,
    /// The inbound transfer has already been redeemed.
    TransferAlreadyRedeemed = 82,
    /// Message target chain does not match this manager's chain ID.
    InvalidTargetChain = 83,
    /// Transfer has not reached the attestation threshold yet.
    TransferNotApproved = 84,
}

/// Errors returned by the Wormhole Transceiver contract.
///
/// Variants are grouped by numeric range:
/// - `1..=3`: initialization and authorization
/// - `10..=15`: peer registration and chain ID validation
/// - `20..=22`: Wormhole core interactions (verify, post)
/// - `30..=36`: NTT message decoding and attestation dispatch
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[contracterror]
#[repr(u32)]
pub enum TransceiverError {
    /// Required contract state has not been initialized.
    NotInitialized = 1,
    /// Caller is not authorized for the requested operation.
    Unauthorized = 3,
    /// Peer chain ID cannot be zero.
    InvalidPeerChainIdZero = 10,
    /// Peer emitter address is the zero address.
    InvalidPeerZeroAddress = 11,
    /// A peer is already registered for the given chain ID; use `update_peer` instead.
    PeerAlreadySet = 12,
    /// No peer is registered for the given chain ID.
    PeerNotFound = 13,
    /// The peer for this chain is registered but currently disabled.
    PeerDisabled = 14,
    /// Chain ID exceeds `u16::MAX` (the Wormhole chain ID range).
    ChainIdTooLarge = 15,
    /// Wormhole core rejected the VAA — invalid format, signatures, or guardian set.
    WormholeVerificationFailed = 20,
    /// Wormhole core rejected the outbound `post_message` call.
    WormholePostFailed = 22,
    /// Decoded NTT message prefix did not match the Wormhole transceiver magic bytes.
    InvalidTransceiverPrefix = 30,
    /// NTT message payload is shorter than the minimum transceiver message length.
    MessageTooShort = 31,
    /// Outbound manager payload exceeds the maximum encodable length (`u16::MAX`).
    PayloadTooLong = 32,
    /// Recipient manager ID in the decoded message does not match this transceiver's manager.
    UnexpectedRecipientManager = 33,
    /// A VAA with the same `(emitter_chain, emitter_address, sequence)` has already been consumed.
    ReplayDetected = 34,
    /// VAA emitter address does not match the registered peer emitter for its chain.
    UnexpectedEmitter = 35,
    /// The downstream manager rejected the forwarded attestation.
    ManagerRejectedMessage = 36,
}
