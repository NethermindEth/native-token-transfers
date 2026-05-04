use soroban_sdk::{contracttype, Bytes, BytesN, Env};
use wormhole_soroban_client::BytesReader;

use crate::constants::WH_TRANSCEIVER_PREFIX;
use crate::errors::TransceiverError;

/// Wormhole transceiver envelope wrapping a manager payload.
///
/// Receivers verify the envelope, route the inner `manager_payload` to their
/// local NTT manager, and protect against replay using `(emitter_chain,
/// emitter_address, sequence)` from the carrying VAA.
///
/// # Wire Format
/// - `[4]`  prefix: [`WH_TRANSCEIVER_PREFIX`]
/// - `[32]` source_manager
/// - `[32]` recipient_manager
/// - `[2]`  manager_payload_len (big-endian)
/// - `[var]` manager_payload
/// - `[2]`  transceiver_payload_len (big-endian)
/// - `[var]` transceiver_payload
///
/// `transceiver_payload` is reserved by the cross-chain protocol for
/// transceiver-private metadata (see EVM `TransceiverStructs.sol`,
/// Solana `ntt-messages::transceiver`, Sui `ntt_common::transceiver_message`).
/// The Wormhole transceiver in this workspace does not populate it today,
/// but the field is part of the wire format and is preserved on
/// encode/decode to stay byte-compatible with peer chains.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransceiverMessage {
    /// Manager that produced the underlying NTT message.
    pub source_manager: BytesN<32>,
    /// Manager expected to consume `manager_payload`.
    pub recipient_manager: BytesN<32>,
    /// Opaque payload destined for the recipient manager.
    pub manager_payload: Bytes,
    /// Optional transceiver-private payload (currently always empty for the
    /// Wormhole transceiver). Preserved across encode/decode for cross-chain
    /// wire compatibility.
    pub transceiver_payload: Bytes,
}

impl TransceiverMessage {
    /// Minimum encoded size: prefix + two managers + two length prefixes.
    pub const MIN_SIZE: u32 = 4 + 32 + 32 + 2 + 2;

    /// Serializes to the cross-chain wire format.
    ///
    /// Returns `PayloadTooLong` if either `manager_payload` or
    /// `transceiver_payload` exceeds `u16::MAX`.
    pub fn to_bytes(&self, env: &Env) -> Result<Bytes, TransceiverError> {
        if self.manager_payload.len() > u16::MAX as u32
            || self.transceiver_payload.len() > u16::MAX as u32
        {
            return Err(TransceiverError::PayloadTooLong);
        }

        let mut buf = Bytes::from_array(env, &WH_TRANSCEIVER_PREFIX);
        buf.extend_from_array(&self.source_manager.to_array());
        buf.extend_from_array(&self.recipient_manager.to_array());

        buf.extend_from_array(&(self.manager_payload.len() as u16).to_be_bytes());
        buf.append(&self.manager_payload);

        buf.extend_from_array(&(self.transceiver_payload.len() as u16).to_be_bytes());
        buf.append(&self.transceiver_payload);

        Ok(buf)
    }

    /// Deserializes from the cross-chain wire format.
    ///
    /// Returns `MessageTooShort` if the input is truncated, or
    /// `InvalidTransceiverPrefix` if the magic bytes don't match.
    pub fn from_bytes(_env: &Env, bytes: &Bytes) -> Result<Self, TransceiverError> {
        if bytes.len() < Self::MIN_SIZE {
            return Err(TransceiverError::MessageTooShort);
        }

        let mut reader = BytesReader::new(bytes);

        let prefix = reader
            .read_u32_be()
            .map_err(|_| TransceiverError::MessageTooShort)?;
        if prefix != u32::from_be_bytes(WH_TRANSCEIVER_PREFIX) {
            return Err(TransceiverError::InvalidTransceiverPrefix);
        }

        let source_manager: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| TransceiverError::MessageTooShort)?;
        let recipient_manager: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| TransceiverError::MessageTooShort)?;

        let manager_payload_len = reader
            .read_u16_be()
            .map_err(|_| TransceiverError::MessageTooShort)? as u32;
        let manager_payload = reader
            .read_bytes(manager_payload_len)
            .map_err(|_| TransceiverError::MessageTooShort)?;

        let transceiver_payload_len = reader
            .read_u16_be()
            .map_err(|_| TransceiverError::MessageTooShort)? as u32;
        let transceiver_payload = reader
            .read_bytes(transceiver_payload_len)
            .map_err(|_| TransceiverError::MessageTooShort)?;

        Ok(Self {
            source_manager,
            recipient_manager,
            manager_payload,
            transceiver_payload,
        })
    }
}
