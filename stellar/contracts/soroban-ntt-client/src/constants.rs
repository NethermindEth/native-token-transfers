//! Protocol-wide constants shared across NTT contracts.
//!
//! Single source of truth for storage TTLs, registry caps, and on-the-wire
//! message prefixes. Wire prefixes are part of the cross-chain ABI: they
//! must agree across implementations and across this crate's versions.

/// TTL threshold in ledgers (~1 day at 5s/ledger) before extending.
///
/// Used for both instance and persistent storage by every contract in the
/// workspace.
pub const TTL_THRESHOLD: u32 = 17280;

/// TTL extension in ledgers (~30 days at 5s/ledger).
///
/// Used for both instance and persistent storage by every contract in the
/// workspace.
pub const TTL_EXTEND: u32 = 17280 * 30;

/// Maximum number of transceivers a single manager can register.
///
/// Bounded by the `u64` bitmap used to track enabled transceivers.
pub const MAX_TRANSCEIVERS: u32 = 64;

/// Default rate-limit refill window in seconds (24 hours).
///
/// Used by the manager as the fallback when no duration is configured in
/// storage. The bucket fully refills over this period.
pub const RATE_LIMIT_DURATION: u64 = 86400;

/// NTT manager message prefix (`0x994E5454`, ASCII `"™NTT"`).
///
/// Identifies a [`NativeTokenTransfer`](crate::messages::NativeTokenTransfer)
/// payload on the wire so receivers can reject foreign payloads early.
pub const NTT_PREFIX: [u8; 4] = [0x99, 0x4E, 0x54, 0x54];

/// Wormhole transceiver envelope prefix (`0x9945FF10`).
///
/// Distinguishes a wrapped NTT manager payload from any other Wormhole VAA
/// payload. Receivers reject mismatched prefixes.
pub const WH_TRANSCEIVER_PREFIX: [u8; 4] = [0x99, 0x45, 0xff, 0x10];

/// `WormholeTransceiverInfo` broadcast prefix (`0x9C23BD3B`).
///
/// Identifies the periodic transceiver-info messages consumed by the NTT
/// Accountant. See `evm/src/libraries/TransceiverStructs.sol`.
pub const BROADCAST_ID_PREFIX: [u8; 4] = [0x9C, 0x23, 0xBD, 0x3B];

/// `WormholeTransceiverRegistration` broadcast prefix (`0x18FC67C2`).
///
/// Identifies the per-peer registration broadcasts consumed by the NTT
/// Accountant. See `evm/src/libraries/TransceiverStructs.sol`.
pub const BROADCAST_PEER_PREFIX: [u8; 4] = [0x18, 0xFC, 0x67, 0xC2];

/// Transceiver-type identifier returned by Wormhole transceiver instances.
///
/// Off-chain tooling reads this to distinguish a Wormhole transceiver from
/// other transceiver kinds. Part of the published protocol surface.
pub const WORMHOLE_TRANSCEIVER_TYPE: [u8; 8] = *b"wormhole";
