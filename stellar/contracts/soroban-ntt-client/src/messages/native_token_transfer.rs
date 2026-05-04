use soroban_sdk::{contracttype, Bytes, BytesN, Env};
use wormhole_soroban_client::BytesReader;

use crate::constants::NTT_PREFIX;
use crate::errors::NttManagerError;
use crate::messages::TrimmedAmount;
use crate::utils::validate_chain_id;

/// Core payload for cross-chain token transfers.
///
/// Inner payload of [`NttManagerMessage`](super::NttManagerMessage). Carries
/// transfer amount, source token, recipient, and destination chain.
///
/// # Wire Format (79+ bytes)
/// - `[4]` prefix: [`NTT_PREFIX`]
/// - `[1]` decimals
/// - `[8]` amount (big-endian)
/// - `[32]` source_token
/// - `[32]` to (recipient)
/// - `[2]` to_chain (big-endian)
/// - `[2]` additional_payload_len (optional)
/// - `[var]` additional_payload (optional)
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NativeTokenTransfer {
    pub amount: TrimmedAmount,
    /// Source token address, left-padded to 32 bytes.
    pub source_token: BytesN<32>,
    /// Recipient address on the destination chain.
    pub to: BytesN<32>,
    /// Destination Wormhole chain ID. Stored as `u32` for Soroban
    /// compatibility, serialized as `u16`.
    pub to_chain: u32,
    /// Optional payload for custom integrations.
    pub additional_payload: Option<Bytes>,
}

impl NativeTokenTransfer {
    /// Minimum message size without additional payload.
    pub const MIN_SIZE: u32 = 4 + 1 + 8 + 32 + 32 + 2;

    /// Serializes to the cross-chain wire format.
    ///
    /// Returns `ChainIdTooLarge` if `to_chain` exceeds `u16::MAX`,
    /// or `PayloadTooLong` if additional payload exceeds 65535 bytes.
    pub fn to_bytes(&self, env: &Env) -> Result<Bytes, NttManagerError> {
        let to_chain = validate_chain_id(self.to_chain).ok_or(NttManagerError::ChainIdTooLarge)?;

        let mut buf = Bytes::from_array(env, &NTT_PREFIX);
        buf.append(&self.amount.to_bytes(env));
        buf.extend_from_array(&self.source_token.to_array());
        buf.extend_from_array(&self.to.to_array());
        buf.extend_from_array(&to_chain.to_be_bytes());

        if let Some(ref payload) = self.additional_payload {
            if payload.len() > u16::MAX as u32 {
                return Err(NttManagerError::PayloadTooLong);
            }
            let len = payload.len() as u16;
            buf.extend_from_array(&len.to_be_bytes());
            buf.append(payload);
        }

        Ok(buf)
    }

    /// Deserializes from the cross-chain wire format using `BytesReader`.
    ///
    /// Returns `MessageTooShort` if the input is truncated, or `InvalidPrefix`
    /// if the magic bytes don't match.
    pub fn from_bytes(_env: &Env, bytes: &Bytes) -> Result<Self, NttManagerError> {
        if bytes.len() < Self::MIN_SIZE {
            return Err(NttManagerError::MessageTooShort);
        }

        let mut reader = BytesReader::new(bytes);

        let prefix = reader
            .read_u32_be()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        if prefix != u32::from_be_bytes(NTT_PREFIX) {
            return Err(NttManagerError::InvalidPrefix);
        }

        let decimals = reader
            .read_u8()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let amount_val = reader
            .read_u64_be()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let amount = TrimmedAmount::new(amount_val, decimals)?;

        let source_token: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let to: BytesN<32> = reader
            .read_bytes_n()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        let to_chain = reader
            .read_u16_be()
            .map_err(|_| NttManagerError::MessageTooShort)? as u32;

        let additional_payload = if reader.remaining() > 0 {
            let len = reader
                .read_u16_be()
                .map_err(|_| NttManagerError::MessageTooShort)? as u32;
            if reader.remaining() < len {
                return Err(NttManagerError::MessageTooShort);
            }
            Some(
                reader
                    .read_bytes(len)
                    .map_err(|_| NttManagerError::MessageTooShort)?,
            )
        } else {
            None
        };

        Ok(Self {
            amount,
            source_token,
            to,
            to_chain,
            additional_payload,
        })
    }
}
