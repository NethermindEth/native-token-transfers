use soroban_ntt_client::{NttManagerError, TrimmedAmount};
use soroban_sdk::{contracttype, Bytes, BytesN, Env};
use wormhole_soroban_client::BytesReader;

/// Core payload for cross-chain token transfers.
///
/// This is the inner payload of `NttManagerMessage`, containing the transfer
/// details: amount, source token, recipient, and destination chain.
///
/// # Wire Format (79+ bytes)
/// - `[4]` prefix: `0x994E5454`
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
    /// Destination Wormhole chain ID. Stored as `u32` for Soroban compatibility,
    /// serialized as `u16`.
    pub to_chain: u32,
    /// Optional payload for custom integrations.
    pub additional_payload: Option<Bytes>,
}

impl NativeTokenTransfer {
    /// NTT message prefix: `0x994E5454` ("™NTT").
    pub const PREFIX: [u8; 4] = [0x99, 0x4E, 0x54, 0x54];

    /// Minimum message size without additional payload.
    pub const MIN_SIZE: u32 = 4 + 1 + 8 + 32 + 32 + 2;

    /// Serializes to the cross-chain wire format.
    ///
    /// Returns `ChainIdTooLarge` if `to_chain` exceeds u16::MAX,
    /// or `PayloadTooLong` if additional payload exceeds 65535 bytes.
    pub fn to_bytes(&self, env: &Env) -> Result<Bytes, NttManagerError> {
        if self.to_chain > u16::MAX as u32 {
            return Err(NttManagerError::ChainIdTooLarge);
        }

        let mut buf = Bytes::from_array(env, &Self::PREFIX);
        buf.append(&self.amount.to_bytes(env));
        buf.extend_from_array(&self.source_token.to_array());
        buf.extend_from_array(&self.to.to_array());
        buf.extend_from_array(&(self.to_chain as u16).to_be_bytes());

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
    /// Returns `NttManagerError::MessageTooShort` if the input is truncated, or
    /// `NttManagerError::InvalidPrefix` if the magic bytes don't match.
    pub fn from_bytes(_env: &Env, bytes: &Bytes) -> Result<Self, NttManagerError> {
        if bytes.len() < Self::MIN_SIZE {
            return Err(NttManagerError::MessageTooShort);
        }

        let mut reader = BytesReader::new(bytes);

        let prefix = reader
            .read_u32_be()
            .map_err(|_| NttManagerError::MessageTooShort)?;
        if prefix != u32::from_be_bytes(Self::PREFIX) {
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

/// Wrapper message that includes sender information and a unique ID.
///
/// This wraps `NativeTokenTransfer` with metadata required for message
/// tracking and attestation across chains.
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
    /// Propagates errors from `NativeTokenTransfer::to_bytes`.
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
    /// Returns `NttManagerError::MessageTooShort` if the input is truncated, or propagates
    /// errors from `NativeTokenTransfer::from_bytes`.
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
    /// Propagates errors from `to_bytes`.
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
