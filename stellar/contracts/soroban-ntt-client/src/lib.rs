#![no_std]

pub mod constants;
pub mod errors;
pub mod events;
pub mod manager;
pub mod messages;
pub mod rate_limit;
pub mod transceiver;
pub mod types;
pub mod utils;
pub mod wormhole_transceiver;

pub use constants::{
    BROADCAST_ID_PREFIX, BROADCAST_PEER_PREFIX, MAX_TRANSCEIVERS, NTT_PREFIX, RATE_LIMIT_DURATION,
    TTL_EXTEND, TTL_THRESHOLD, WH_TRANSCEIVER_PREFIX, WORMHOLE_TRANSCEIVER_TYPE,
};
pub use errors::{NttManagerError, TransceiverError};
pub use events::{
    emit_inbound_transfer_queued, emit_message_already_executed, emit_message_attested_to,
    emit_message_received, emit_message_sent, emit_outbound_transfer_cancelled,
    emit_outbound_transfer_queued, emit_outbound_transfer_rate_limited, emit_peer_set,
    emit_peer_updated, emit_threshold_changed, emit_transceiver_added, emit_transceiver_removed,
    emit_transfer_redeemed, emit_transfer_sent, InboundTransferQueued, MessageAlreadyExecuted,
    MessageAttestedTo, MessageReceived, MessageSent, OutboundTransferCancelled,
    OutboundTransferQueued, OutboundTransferRateLimited, PeerSet, PeerUpdated, ThresholdChanged,
    TransceiverAdded, TransceiverRemoved, TransferRedeemed, TransferSent,
};
pub use manager::{NttManagerClient, NttManagerInterface};
pub use messages::{
    NativeTokenTransfer, NttManagerMessage, TransceiverMessage, TrimmedAmount,
    WormholeTransceiverInfo, WormholeTransceiverRegistration,
};
pub use rate_limit::{RateLimitParams, RateLimiterClient, RateLimiterInterface};
pub use transceiver::{TransceiverClient, TransceiverInterface};
pub use types::{
    AttestationInfo, AttestationResult, InboundQueuedTransfer, Mode, NttManagerPeer,
    OutboundQueuedTransfer, PeerInfo, TransceiverFee, TransferResult,
};
pub use utils::{flatten_call, is_zero_bytes32, sequence_to_message_id, validate_chain_id};
pub use wormhole_transceiver::{WormholeTransceiverClient, WormholeTransceiverInterface};
