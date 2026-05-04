use soroban_sdk::{contracttype, Bytes, Env};

use crate::errors::NttManagerError;

/// Amount normalized to the common NTT decimal domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TrimmedAmount {
    /// Amount value in the trimmed decimal domain.
    pub amount: u64,
    /// Decimal precision used for `amount`.
    pub decimals: u32,
}

impl TrimmedAmount {
    /// Maximum decimal precision allowed in the normalized NTT amount domain.
    pub const MAX_DECIMALS: u8 = 8;

    /// Creates a new trimmed amount.
    ///
    /// Returns `InvalidDecimals` when `decimals > 8`.
    pub fn new(amount: u64, decimals: u8) -> Result<Self, NttManagerError> {
        if decimals > Self::MAX_DECIMALS {
            return Err(NttManagerError::InvalidDecimals);
        }
        Ok(Self {
            amount,
            decimals: decimals as u32,
        })
    }

    /// Trims an amount into the common NTT decimal domain.
    ///
    /// Returns `(trimmed_amount, dust)` where `dust` is the precision lost
    /// during trimming.
    pub fn trim(
        amount: u128,
        from_decimals: u8,
        to_decimals: u8,
    ) -> Result<(Self, u128), NttManagerError> {
        let target_decimals = core::cmp::min(
            Self::MAX_DECIMALS,
            core::cmp::min(from_decimals, to_decimals),
        );

        if from_decimals <= target_decimals {
            let amount_u64 = u64::try_from(amount).map_err(|_| NttManagerError::AmountOverflow)?;
            return Ok((
                Self::new(amount_u64, from_decimals).expect("decimals <= MAX"),
                0,
            ));
        }

        let scale = 10u128.pow((from_decimals - target_decimals) as u32);
        let trimmed = amount / scale;
        let dust = amount % scale;

        let trimmed_u64 = u64::try_from(trimmed).map_err(|_| NttManagerError::AmountOverflow)?;
        Ok((
            Self::new(trimmed_u64, target_decimals).expect("decimals <= MAX"),
            dust,
        ))
    }

    /// Expands this trimmed amount to the requested decimal precision.
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

    /// Trims an amount in place and subtracts the discarded dust.
    pub fn remove_dust(
        amount: &mut u128,
        from_decimals: u8,
        to_decimals: u8,
    ) -> Result<Self, NttManagerError> {
        let (trimmed, dust) = Self::trim(*amount, from_decimals, to_decimals)?;
        *amount -= dust;
        Ok(trimmed)
    }

    /// Serializes to the NTT wire representation: 1 byte decimals + 8 byte amount.
    pub fn to_bytes(&self, env: &Env) -> Bytes {
        let mut buf = [0u8; 9];
        buf[0] = self.decimals as u8;
        buf[1..9].copy_from_slice(&self.amount.to_be_bytes());
        Bytes::from_array(env, &buf)
    }

    /// Returns `true` when the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }

    /// Adds two trimmed amounts, requiring matching decimals.
    pub fn checked_add(&self, other: &Self) -> Result<Self, NttManagerError> {
        if self.decimals != other.decimals {
            return Err(NttManagerError::DecimalMismatch);
        }
        Ok(Self {
            amount: self.amount.saturating_add(other.amount),
            decimals: self.decimals,
        })
    }

    /// Subtracts two trimmed amounts, requiring matching decimals.
    pub fn checked_sub(&self, other: &Self) -> Result<Self, NttManagerError> {
        if self.decimals != other.decimals {
            return Err(NttManagerError::DecimalMismatch);
        }
        Ok(Self {
            amount: self.amount.saturating_sub(other.amount),
            decimals: self.decimals,
        })
    }
}
