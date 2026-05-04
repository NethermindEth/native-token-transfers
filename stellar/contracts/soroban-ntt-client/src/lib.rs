#![no_std]

pub mod constants;
pub mod errors;
pub mod manager;
pub mod messages;
pub mod rate_limit;
pub mod transceiver;
pub mod types;
pub mod utils;
pub mod wormhole_transceiver;

pub use constants::{
    BROADCAST_ID_PREFIX, BROADCAST_PEER_PREFIX, MAX_TRANSCEIVERS, NTT_PREFIX, TTL_EXTEND,
    TTL_THRESHOLD, WH_TRANSCEIVER_PREFIX,
};
pub use errors::{NttManagerError, TransceiverError};
pub use manager::{NttManagerClient, NttManagerInterface};
pub use messages::{NativeTokenTransfer, NttManagerMessage, TransceiverMessage, TrimmedAmount};
pub use rate_limit::{RateLimitParams, RateLimiterClient, RateLimiterInterface};
pub use transceiver::{TransceiverClient, TransceiverInterface};
pub use types::{
    AttestationInfo, AttestationResult, InboundQueuedTransfer, Mode, NttManagerPeer,
    OutboundQueuedTransfer, PeerInfo, TransferResult,
};
pub use utils::{address_to_bytes32, bytes32_to_address, sequence_to_message_id, validate_chain_id};
pub use wormhole_transceiver::{WormholeTransceiverClient, WormholeTransceiverInterface};
