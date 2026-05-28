//! Shared data types passed across the NTT manager / transceiver interfaces.
//!
//! These describe state and return shapes — not behaviour. `#[contracttype]`
//! field order is the on-chain layout, so reordering breaks state.

use soroban_sdk::{contracttype, Address, Bytes, BytesN};

use crate::messages::TrimmedAmount;
use crate::rate_limit::RateLimitParams;

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

/// Result of a transfer operation, returned to the caller.
///
/// Contains the sequence number for tracking, whether the transfer was queued
/// due to rate limiting, and the message digest for verification.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransferResult {
    /// Unique sequence number assigned to this transfer.
    pub sequence: u64,
    /// Whether this transfer was queued (`true`) or sent immediately (`false`).
    pub queued: bool,
    /// Keccak-256 digest of the NTT message payload.
    pub digest: BytesN<32>,
}

impl TransferResult {
    /// Transfer dispatched to transceivers in the same call.
    pub fn immediate(sequence: u64, digest: BytesN<32>) -> Self {
        Self { sequence, queued: false, digest }
    }

    /// Transfer queued by the rate limiter and pending later release.
    pub fn queued(sequence: u64, digest: BytesN<32>) -> Self {
        Self { sequence, queued: true, digest }
    }
}

/// Delivery-fee quote for a single enabled transceiver.
///
/// `fee` is `None` when that transceiver could not produce a quote, so a
/// caller sees exactly which transceiver is unavailable rather than losing
/// the whole result to one failure. Sum the `Some` values for the total cost
/// of dispatching a transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransceiverFee {
    /// The enabled transceiver this quote came from.
    pub transceiver: Address,
    /// Delivery fee in stroops, or `None` if the transceiver's quote failed.
    pub fee: Option<i128>,
}

/// Result of processing an attestation from a transceiver.
///
/// Indicates whether the attestation threshold was met, whether tokens
/// were released, and whether the transfer was queued due to rate limiting.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct AttestationResult {
    /// Whether the attestation threshold is now met.
    pub approved: bool,
    /// Whether tokens were released to the recipient.
    pub executed: bool,
    /// Whether the transfer was queued due to rate limiting.
    pub queued: bool,
}

impl AttestationResult {
    /// Threshold met and tokens released in the same call.
    pub const fn executed() -> Self {
        Self { approved: true, executed: true, queued: false }
    }

    /// Threshold met but the transfer was queued by the rate limiter.
    pub const fn queued() -> Self {
        Self { approved: true, executed: false, queued: true }
    }

    /// Threshold not yet met; waiting for more attestations.
    pub const fn not_approved() -> Self {
        Self { approved: false, executed: false, queued: false }
    }
}

/// Outbound transfer currently queued by the rate limiter.
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
    /// Token contract address encoded as bytes32.
    pub source_token: BytesN<32>,
    /// Ledger timestamp when the transfer becomes releasable.
    pub release_timestamp: u64,
    /// Optional custom payload attached to the transfer.
    pub additional_payload: Option<Bytes>,
}

/// Inbound transfer currently queued by the rate limiter.
#[derive(Clone, Debug)]
#[contracttype]
pub struct InboundQueuedTransfer {
    /// Recipient address on this chain.
    pub recipient: Address,
    /// Amount in local token decimals.
    pub amount: i128,
    /// Original trimmed amount from the wire format.
    pub trimmed_amount: u64,
    /// Ledger timestamp when the transfer becomes releasable.
    pub release_timestamp: u64,
}

/// Stored attestation state for an inbound message digest.
#[derive(Clone, Debug)]
#[contracttype]
pub struct AttestationInfo {
    /// Whether the message has already been executed.
    pub executed: bool,
    /// Bitmap of transceiver indices that have attested.
    pub attested_transceivers: u64,
}

/// Registration entry for a peer Wormhole transceiver on another chain.
///
/// Stored per-chain and consulted on both outbound dispatch (to locate
/// the recipient transceiver) and inbound verification (to reject VAAs
/// from unexpected emitters).
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct PeerInfo {
    /// 32-byte Wormhole emitter address of the peer transceiver.
    pub emitter: BytesN<32>,
    /// Whether this peer is currently accepting messages in either direction.
    pub enabled: bool,
}
