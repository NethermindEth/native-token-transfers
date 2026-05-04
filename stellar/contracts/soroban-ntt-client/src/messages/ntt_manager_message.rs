use soroban_sdk::{contracttype, Bytes, BytesN, Env};
use wormhole_soroban_client::BytesReader;

use crate::errors::NttManagerError;
use crate::messages::NativeTokenTransfer;

/// Wrapper message that includes sender information and a unique ID.
///
/// Wraps [`NativeTokenTransfer`] with metadata required for message tracking
/// and attestation across chains.
///
/// # Wire Format (66+ bytes)
/// - `[32]` id (unique message identifier)
/// - `[32]` sender (original sender, left-padded)
/// - `[2]` payload_len (big-endian)
/// - `[var]` payload (encoded `NativeTokenTransfer`)
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NttManagerMessage {
    /// Unique message identifier, typically derived from sequence number.
    pub id: BytesN<32>,
    /// Original sender address, left-padded to 32 bytes.
    pub sender: BytesN<32>,
    /// The transfer payload.
    pub payload: NativeTokenTransfer,
}

impl NttManagerMessage {
    /// Minimum message size: 32 (id) + 32 (sender) + 2 (len).
    pub const MIN_SIZE: u32 = 32 + 32 + 2;

    /// Serializes to the cross-chain wire format.
    ///
    /// Propagates errors from [`NativeTokenTransfer::to_bytes`].
    pub fn to_bytes(&self, env: &Env) -> Result<Bytes, NttManagerError> {
        let mut buf = Bytes::from_array(env, &self.id.to_array());
        buf.extend_from_array(&self.sender.to_array());

        let payload_bytes = self.payload.to_bytes(env)?;
        let payload_len = payload_bytes.len() as u16;
        buf.extend_from_array(&payload_len.to_be_bytes());
        buf.append(&payload_bytes);

        Ok(buf)
    }

    /// Deserializes from the cross-chain wire format.
    ///
    /// Returns `MessageTooShort` if the input is truncated, or propagates
    /// errors from [`NativeTokenTransfer::from_bytes`].
    pub fn from_bytes(env: &Env, bytes: &Bytes) -> Result<Self, NttManagerError> {
        if bytes.len() < Self::MIN_SIZE {
            return Err(NttManagerError::MessageTooShort);
        }

        let mut reader = BytesReader::new(bytes);

        let id: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let sender: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| NttManagerError::MessageTooShort)?;

        let payload_len = reader
            .read_u16_be()
            .map_err(|_| NttManagerError::MessageTooShort)? as u32;
        if reader.remaining() < payload_len {
            return Err(NttManagerError::MessageTooShort);
        }

        let payload_bytes = reader
            .read_bytes(payload_len)
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let payload = NativeTokenTransfer::from_bytes(env, &payload_bytes)?;

        Ok(Self {
            id,
            sender,
            payload,
        })
    }

    /// Computes the message digest for attestation tracking.
    ///
    /// Propagates errors from [`Self::to_bytes`].
    pub fn compute_digest(
        &self,
        env: &Env,
        source_chain: u16,
    ) -> Result<BytesN<32>, NttManagerError> {
        let mut data = Bytes::from_array(env, &source_chain.to_be_bytes());
        data.append(&self.to_bytes(env)?);
        Ok(env.crypto().keccak256(&data).into())
    }
}
