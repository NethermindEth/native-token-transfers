use soroban_sdk::{contracttype, Bytes, Env};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TrimmedAmount {
    pub amount: u64,
    pub decimals: u32,
}

impl TrimmedAmount {
    pub const MAX_DECIMALS: u8 = 8;

    pub fn new(amount: u64, decimals: u8) -> Self {
        assert!(decimals <= Self::MAX_DECIMALS);
        Self {
            amount,
            decimals: decimals as u32,
        }
    }

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

    pub fn remove_dust(amount: &mut u128, from_decimals: u8, to_decimals: u8) -> Self {
        let (trimmed, dust) = Self::trim(*amount, from_decimals, to_decimals);
        *amount -= dust;
        trimmed
    }

    pub fn to_bytes(&self, env: &Env) -> Bytes {
        let mut buf = Bytes::new(env);
        buf.push_back(self.decimals as u8);
        buf.append(&Bytes::from_array(env, &self.amount.to_be_bytes()));
        buf
    }

    pub fn from_bytes(bytes: &Bytes, offset: u32) -> Self {
        let decimals = bytes.get(offset).expect("missing decimals");
        let mut amount_bytes = [0u8; 8];
        for i in 0..8 {
            amount_bytes[i] = bytes.get(offset + 1 + i as u32).expect("missing amount byte");
        }
        Self {
            amount: u64::from_be_bytes(amount_bytes),
            decimals: decimals as u32,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }

    pub fn saturating_add(&self, other: &Self) -> Self {
        assert_eq!(self.decimals, other.decimals, "decimal mismatch");
        Self {
            amount: self.amount.saturating_add(other.amount),
            decimals: self.decimals,
        }
    }

    pub fn saturating_sub(&self, other: &Self) -> Self {
        assert_eq!(self.decimals, other.decimals, "decimal mismatch");
        Self {
            amount: self.amount.saturating_sub(other.amount),
            decimals: self.decimals,
        }
    }
}
