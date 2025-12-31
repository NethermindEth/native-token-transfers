use soroban_sdk::{contracttype, Address, BytesN, Env};

use crate::errors::NttManagerError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
#[repr(u32)]
pub enum Mode {
    Locking = 0,
    Burning = 1,
}

impl Mode {
    pub fn is_locking(&self) -> bool {
        matches!(self, Mode::Locking)
    }

    pub fn is_burning(&self) -> bool {
        matches!(self, Mode::Burning)
    }
}


#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    PendingAdmin,
    Token,
    TokenDecimals,
    Mode,
    ChainId,
    Paused,
    Threshold,
    NextSequence,
    Version,
    TransceiverCount,
    EnabledBitmap,
    RateLimitDuration,
    OutboundRateLimit,

    Peer(u32),
    Transceiver(u32),
    TransceiverIndex(Address),
    Attestation(BytesN<32>),
    OutboundQueue(u64),
    InboundQueue(BytesN<32>),

    Reentering,
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct NttConfig {
    pub mode: Mode,
    pub token: Address,
    pub token_decimals: u32,
    pub chain_id: u32,
    pub admin: Address,
    pub paused: bool,
    pub threshold: u32,
    pub rate_limit_duration: u64,
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("admin not set")
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), NttManagerError> {
    caller.require_auth();
    let admin = get_admin(env);
    if *caller != admin {
        return Err(NttManagerError::Unauthorized);
    }
    Ok(())
}
