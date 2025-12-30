use soroban_sdk::{contracttype, Bytes, BytesN, Env};
use wormhole_soroban_client::BytesReader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum Error {
    MessageTooShort = 1,
    InvalidPrefix = 2,
}

/// Normalized token amount for cross-chain compatibility.
///
/// All cross-chain amounts are capped at 8 decimals to ensure consistent
/// representation across chains with different native decimal precisions.
/// The `decimals` field is stored as `u32` due to Soroban's `#[contracttype]`
/// constraints, but is logically limited to 0-8.
///
/// # Wire Format
/// Serializes to exactly 9 bytes: 1 byte for decimals + 8 bytes for amount (big-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TrimmedAmount {
    pub amount: u64,
    pub decimals: u32,
}

impl TrimmedAmount {
    /// Maximum decimals allowed for cross-chain transfers.
    pub const MAX_DECIMALS: u8 = 8;

    /// Creates a new `TrimmedAmount`.
    ///
    /// # Panics
    /// Panics if `decimals > 8`.
    pub fn new(amount: u64, decimals: u8) -> Self {
        assert!(decimals <= Self::MAX_DECIMALS);
        Self {
            amount,
            decimals: decimals as u32,
        }
    }

    /// Trims an amount from source decimals to the minimum of (8, from, to).
    ///
    /// Returns `(trimmed_amount, dust)` where dust is the precision lost to trimming.
    /// If `from_decimals` is already at or below the target precision, returns the
    /// amount unchanged with zero dust.
    ///
    /// # Example
    /// `trim(1_234_567_890, 9, 6)` returns `(TrimmedAmount{1_234_567, 6}, 890)`
    pub fn trim(amount: u128, from_decimals: u8, to_decimals: u8) -> (Self, u128) {
        let target_decimals = core::cmp::min(
            Self::MAX_DECIMALS,
            core::cmp::min(from_decimals, to_decimals),
        );

        if from_decimals <= target_decimals {
            return (Self::new(amount as u64, from_decimals), 0);
        }

        let scale = 10u128.pow((from_decimals - target_decimals) as u32);
        let trimmed = amount / scale;
        let dust = amount % scale;

        (Self::new(trimmed as u64, target_decimals), dust)
    }

    /// Expands the trimmed amount back to the target decimal precision.
    ///
    /// If `to_decimals` exceeds the stored decimals, multiplies by the appropriate
    /// power of 10. If `to_decimals` is less, divides (losing precision).
    ///
    /// # Example
    /// `TrimmedAmount{1_234, 6}.untrim(9)` returns `1_234_000`
    pub fn untrim(&self, to_decimals: u8) -> u128 {
        let self_decimals = self.decimals as u8;
        if to_decimals >= self_decimals {
            let scale = 10u128.pow((to_decimals - self_decimals) as u32);
            (self.amount as u128) * scale
        } else {
            let scale = 10u128.pow((self_decimals - to_decimals) as u32);
            (self.amount as u128) / scale
        }
    }

    /// Trims an amount in place, removing dust from the original value.
    ///
    /// Modifies `amount` to subtract the dust lost during trimming, then returns
    /// the trimmed representation. Useful when the caller needs the adjusted
    /// amount for token operations.
    pub fn remove_dust(amount: &mut u128, from_decimals: u8, to_decimals: u8) -> Self {
        let (trimmed, dust) = Self::trim(*amount, from_decimals, to_decimals);
        *amount -= dust;
        trimmed
    }

    /// Serializes to 9 bytes: 1 byte decimals + 8 bytes big-endian amount.
    pub fn to_bytes(&self, env: &Env) -> Bytes {
        let mut buf = Bytes::new(env);
        buf.push_back(self.decimals as u8);
        buf.append(&Bytes::from_array(env, &self.amount.to_be_bytes()));
        buf
    }

    /// Deserializes from bytes at the given offset.
    ///
    /// # Panics
    /// Panics if there are fewer than 9 bytes available starting at `offset`.
    pub fn from_bytes(bytes: &Bytes, offset: u32) -> Self {
        let decimals = bytes.get(offset).expect("missing decimals");
        let mut amount_bytes = [0u8; 8];
        for i in 0..8 {
            amount_bytes[i] = bytes
                .get(offset + 1 + i as u32)
                .expect("missing amount byte");
        }
        Self {
            amount: u64::from_be_bytes(amount_bytes),
            decimals: decimals as u32,
        }
    }

    /// Returns `true` if the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }

    /// Saturating addition. Both operands must have matching decimals.
    ///
    /// # Panics
    /// Panics if `self.decimals != other.decimals`.
    pub fn saturating_add(&self, other: &Self) -> Self {
        assert_eq!(self.decimals, other.decimals, "decimal mismatch");
        Self {
            amount: self.amount.saturating_add(other.amount),
            decimals: self.decimals,
        }
    }

    /// Saturating subtraction. Both operands must have matching decimals.
    ///
    /// # Panics
    /// Panics if `self.decimals != other.decimals`.
    pub fn saturating_sub(&self, other: &Self) -> Self {
        assert_eq!(self.decimals, other.decimals, "decimal mismatch");
        Self {
            amount: self.amount.saturating_sub(other.amount),
            decimals: self.decimals,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct NativeTokenTransfer {
    pub amount: TrimmedAmount,
    pub source_token: BytesN<32>,
    pub to: BytesN<32>,
    pub to_chain: u32,
    pub additional_payload: Option<Bytes>,
}

impl NativeTokenTransfer {
    pub const PREFIX: [u8; 4] = [0x99, 0x4E, 0x54, 0x54];
    pub const MIN_SIZE: u32 = 4 + 1 + 8 + 32 + 32 + 2;

    pub fn to_bytes(&self, env: &Env) -> Bytes {
        let mut buf = Bytes::new(env);

        buf.append(&Bytes::from_array(env, &Self::PREFIX));
        buf.append(&self.amount.to_bytes(env));
        buf.append(&Bytes::from_array(env, &self.source_token.to_array()));
        buf.append(&Bytes::from_array(env, &self.to.to_array()));
        buf.append(&Bytes::from_array(env, &(self.to_chain as u16).to_be_bytes()));

        if let Some(ref payload) = self.additional_payload {
            let len = payload.len() as u16;
            buf.append(&Bytes::from_array(env, &len.to_be_bytes()));
            buf.append(payload);
        }

        buf
    }

    pub fn from_bytes(_env: &Env, bytes: &Bytes) -> Result<Self, Error> {
        if bytes.len() < Self::MIN_SIZE {
            return Err(Error::MessageTooShort);
        }

        let mut reader = BytesReader::new(bytes);

        let prefix = reader.read_u32_be().map_err(|_| Error::MessageTooShort)?;
        if prefix != u32::from_be_bytes(Self::PREFIX) {
            return Err(Error::InvalidPrefix);
        }

        let decimals = reader.read_u8().map_err(|_| Error::MessageTooShort)?;
        let amount_val = reader.read_u64_be().map_err(|_| Error::MessageTooShort)?;
        let amount = TrimmedAmount::new(amount_val, decimals);

        let source_token: BytesN<32> = reader.read_bytes_n().map_err(|_| Error::MessageTooShort)?;
        let to: BytesN<32> = reader.read_bytes_n().map_err(|_| Error::MessageTooShort)?;
        let to_chain = reader.read_u16_be().map_err(|_| Error::MessageTooShort)? as u32;

        let additional_payload = if reader.remaining() > 0 {
            let len = reader.read_u16_be().map_err(|_| Error::MessageTooShort)? as u32;
            if reader.remaining() < len {
                return Err(Error::MessageTooShort);
            }
            Some(reader.read_bytes(len).map_err(|_| Error::MessageTooShort)?)
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
