//! Type-safe instance storage wrapper with automatic TTL extension.

use soroban_sdk::{Address, Env};

use crate::constants::{INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, RATE_LIMIT_DURATION};
use crate::errors::NttManagerError;
use crate::state::{DataKey, Mode};

/// Wraps instance storage with automatic TTL extension and typed accessors.
///
/// Extends TTL once on construction, eliminating scattered `extend_instance_ttl` calls.
/// Getters return `Result` for required fields, `Option` for optional fields, and
/// direct values with defaults for fields that have sensible fallbacks.
pub struct InstanceStorage<'a> {
    env: &'a Env,
}

impl<'a> InstanceStorage<'a> {
    /// Creates the accessor, extending instance TTL.
    #[inline]
    pub fn new(env: &'a Env) -> Self {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
        Self { env }
    }

    // --- Required getters (fail if uninitialized) ---

    #[inline]
    pub fn admin(&self) -> Result<Address, NttManagerError> {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(NttManagerError::NotInitialized)
    }

    #[inline]
    pub fn token(&self) -> Result<Address, NttManagerError> {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(NttManagerError::NotInitialized)
    }

    #[inline]
    pub fn token_decimals(&self) -> Result<u32, NttManagerError> {
        self.env
            .storage()
            .instance()
            .get(&DataKey::TokenDecimals)
            .ok_or(NttManagerError::NotInitialized)
    }

    #[inline]
    pub fn mode(&self) -> Result<Mode, NttManagerError> {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Mode)
            .ok_or(NttManagerError::NotInitialized)
    }

    #[inline]
    pub fn chain_id(&self) -> Result<u32, NttManagerError> {
        self.env
            .storage()
            .instance()
            .get(&DataKey::ChainId)
            .ok_or(NttManagerError::NotInitialized)
    }

    // --- Optional getters ---

    #[inline]
    pub fn pending_admin(&self) -> Option<Address> {
        self.env.storage().instance().get(&DataKey::PendingAdmin)
    }

    #[inline]
    pub fn pauser(&self) -> Option<Address> {
        self.env.storage().instance().get(&DataKey::Pauser)
    }

    // --- Default-value getters ---

    #[inline]
    pub fn is_paused(&self) -> bool {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    #[inline]
    pub fn threshold(&self) -> u32 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or(0)
    }

    #[inline]
    pub fn next_sequence(&self) -> u64 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::NextSequence)
            .unwrap_or(1)
    }

    #[inline]
    pub fn version(&self) -> u32 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::Version)
            .unwrap_or(1)
    }

    #[inline]
    pub fn transceiver_count(&self) -> u32 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::TransceiverCount)
            .unwrap_or(0)
    }

    #[inline]
    pub fn enabled_bitmap(&self) -> u64 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::EnabledBitmap)
            .unwrap_or(0)
    }

    /// Defaults to 24 hours (86400 seconds).
    #[inline]
    pub fn rate_limit_duration(&self) -> u64 {
        self.env
            .storage()
            .instance()
            .get(&DataKey::RateLimitDuration)
            .unwrap_or(RATE_LIMIT_DURATION)
    }

    // --- Setters ---

    #[inline]
    pub fn set_admin(&self, admin: &Address) {
        self.env.storage().instance().set(&DataKey::Admin, admin);
    }

    #[inline]
    pub fn set_pending_admin(&self, pending_admin: &Address) {
        self.env
            .storage()
            .instance()
            .set(&DataKey::PendingAdmin, pending_admin);
    }

    #[inline]
    pub fn remove_pending_admin(&self) {
        self.env.storage().instance().remove(&DataKey::PendingAdmin);
    }

    /// Pass `None` to remove the pauser.
    #[inline]
    pub fn set_pauser(&self, pauser: Option<&Address>) {
        match pauser {
            Some(p) => self.env.storage().instance().set(&DataKey::Pauser, p),
            None => self.env.storage().instance().remove(&DataKey::Pauser),
        }
    }

    #[inline]
    pub fn set_paused(&self, paused: bool) {
        self.env.storage().instance().set(&DataKey::Paused, &paused);
    }

    #[inline]
    pub fn set_threshold(&self, threshold: u32) {
        self.env
            .storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
    }

    #[inline]
    pub fn set_next_sequence(&self, sequence: u64) {
        self.env
            .storage()
            .instance()
            .set(&DataKey::NextSequence, &sequence);
    }

    #[inline]
    pub fn set_transceiver_count(&self, count: u32) {
        self.env
            .storage()
            .instance()
            .set(&DataKey::TransceiverCount, &count);
    }

    #[inline]
    pub fn set_enabled_bitmap(&self, bitmap: u64) {
        self.env
            .storage()
            .instance()
            .set(&DataKey::EnabledBitmap, &bitmap);
    }

    // --- Compound operations ---

    /// Atomically gets and increments the sequence counter. Starts at 1.
    #[inline]
    pub fn use_sequence(&self) -> u64 {
        let current = self.next_sequence();
        self.set_next_sequence(current + 1);
        current
    }

    /// Authenticates `caller` and verifies they are the admin.
    pub fn require_admin(&self, caller: &Address) -> Result<(), NttManagerError> {
        caller.require_auth();
        if *caller != self.admin()? {
            return Err(NttManagerError::Unauthorized);
        }
        Ok(())
    }

    /// Authenticates `caller` and verifies they are admin or pauser.
    pub fn require_admin_or_pauser(&self, caller: &Address) -> Result<(), NttManagerError> {
        caller.require_auth();
        if *caller == self.admin()? {
            return Ok(());
        }
        if let Some(pauser) = self.pauser() {
            if *caller == pauser {
                return Ok(());
            }
        }
        Err(NttManagerError::InvalidPauser)
    }

    /// Returns `ContractPaused` error if paused.
    #[inline]
    pub fn require_not_paused(&self) -> Result<(), NttManagerError> {
        if self.is_paused() {
            return Err(NttManagerError::ContractPaused);
        }
        Ok(())
    }
}
