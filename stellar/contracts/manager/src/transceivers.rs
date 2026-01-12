//! Transceiver registry for managing cross-chain message relayers.
//!
//! Transceivers are responsible for sending and receiving messages across chains.
//! This module provides a bitmap-based registry that tracks up to 64 transceivers,
//! along with threshold-based attestation requirements.
use crate::{
    constants::MAX_TRANSCEIVERS,
    errors::NttManagerError,
    state::{extend_persistent_ttl, DataKey},
};
use soroban_sdk::{contracttype, Address, Env, Vec};

/// Metadata for a registered transceiver.
///
/// Once registered, a transceiver's index is permanent and never reused,
/// even if the transceiver is later disabled.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransceiverInfo {
    /// Contract address of the transceiver.
    pub address: Address,
    /// Whether the transceiver is currently enabled for attestations.
    pub enabled: bool,
    /// Permanent index in the bitmap (0-63). Never reused after assignment.
    pub index: u32,
}

/// Retrieves the bitmap of currently enabled transceivers.
///
/// Returns an empty bitmap if not initialized.
pub fn get_enabled_bitmap(env: &Env) -> Bitmap {
    let raw: u64 = env
        .storage()
        .instance()
        .get(&DataKey::EnabledBitmap)
        .unwrap_or(0);
    Bitmap(raw)
}

/// Retrieves the current attestation threshold.
///
/// Returns 0 if no transceivers have been registered.
pub fn get_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::Threshold)
        .unwrap_or(0)
}

/// Retrieves transceiver info by its permanent index.
///
/// Returns `None` if no transceiver exists at the given index.
pub fn get_transceiver(env: &Env, index: u32) -> Option<TransceiverInfo> {
    env.storage().persistent().get(&DataKey::Transceiver(index))
}

/// Looks up a transceiver's index by its contract address.
///
/// Returns `None` if the address is not registered.
pub fn get_transceiver_index(env: &Env, address: &Address) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&DataKey::TransceiverIndex(address.clone()))
}

/// Checks whether a transceiver is currently enabled.
///
/// Returns `false` if the address is not registered or is disabled.
pub fn is_transceiver_enabled(env: &Env, address: &Address) -> bool {
    if let Some(index) = get_transceiver_index(env, address) {
        if let Some(info) = get_transceiver(env, index) {
            return info.enabled;
        }
    }
    false
}

/// Returns a list of all currently enabled transceiver addresses.
///
/// Iterates through all registered transceivers and filters by the enabled bitmap.
pub fn get_enabled_transceivers(env: &Env) -> Vec<Address> {
    let bitmap = get_enabled_bitmap(env);
    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::TransceiverCount)
        .unwrap_or(0);

    let mut result = Vec::new(env);
    for i in 0..count {
        if bitmap.is_set(i as u8) {
            if let Some(info) = get_transceiver(env, i) {
                result.push_back(info.address);
            }
        }
    }
    result
}

/// Validates threshold invariants after registry modifications.
///
/// Enforces:
/// - INV-023: `threshold <= enabled_count`
/// - INV-024: `threshold > 0` when transceivers exist
pub fn check_threshold_invariants(env: &Env) -> Result<(), NttManagerError> {
    let threshold = get_threshold(env);
    let enabled_count = get_enabled_bitmap(env).count_ones() as u32;

    if threshold > enabled_count {
        return Err(NttManagerError::ThresholdTooHigh);
    }

    if enabled_count > 0 && threshold == 0 {
        return Err(NttManagerError::ZeroThreshold);
    }

    Ok(())
}

/// Registers a new transceiver or re-enables a disabled one.
///
/// If the transceiver was previously registered but disabled, it will be re-enabled
/// at its original index. New transceivers are assigned the next available index.
/// Automatically sets threshold to 1 when the first transceiver is registered.
///
/// Returns the transceiver's index on success.
///
/// # Errors
/// - `TransceiverAlreadyEnabled` if transceiver is already active
/// - `MaxTransceiversReached` if 64 transceivers already registered
/// - `TransceiverNotRegistered` if index lookup fails (internal error)
pub fn set_transceiver(env: &Env, transceiver: Address) -> Result<u32, NttManagerError> {
    let existing_index = get_transceiver_index(env, &transceiver);

    if let Some(index) = existing_index {
        let mut info: TransceiverInfo = env
            .storage()
            .persistent()
            .get(&DataKey::Transceiver(index))
            .ok_or(NttManagerError::TransceiverNotRegistered)?;

        if info.enabled {
            return Err(NttManagerError::TransceiverAlreadyEnabled);
        }

        info.enabled = true;
        env.storage()
            .persistent()
            .set(&DataKey::Transceiver(index), &info);
        extend_persistent_ttl(env, &DataKey::Transceiver(index));

        let mut bitmap = get_enabled_bitmap(env);
        bitmap.set(index as u8);
        env.storage()
            .instance()
            .set(&DataKey::EnabledBitmap, &bitmap.raw());

        check_threshold_invariants(env)?;
        return Ok(index);
    }

    let count: u32 = env
        .storage()
        .instance()
        .get(&DataKey::TransceiverCount)
        .unwrap_or(0);

    if count >= MAX_TRANSCEIVERS {
        return Err(NttManagerError::MaxTransceiversReached);
    }

    let index = count;
    let info = TransceiverInfo {
        address: transceiver.clone(),
        enabled: true,
        index,
    };

    env.storage()
        .persistent()
        .set(&DataKey::Transceiver(index), &info);
    extend_persistent_ttl(env, &DataKey::Transceiver(index));
    env.storage()
        .persistent()
        .set(&DataKey::TransceiverIndex(transceiver.clone()), &index);
    extend_persistent_ttl(env, &DataKey::TransceiverIndex(transceiver));
    env.storage()
        .instance()
        .set(&DataKey::TransceiverCount, &(count + 1));

    let mut bitmap = get_enabled_bitmap(env);
    bitmap.set(index as u8);
    env.storage()
        .instance()
        .set(&DataKey::EnabledBitmap, &bitmap.raw());

    let threshold: u32 = env
        .storage()
        .instance()
        .get(&DataKey::Threshold)
        .unwrap_or(0);
    if threshold == 0 {
        env.storage().instance().set(&DataKey::Threshold, &1u32);
    }

    check_threshold_invariants(env)?;

    Ok(index)
}

/// Disables a transceiver, excluding it from attestation voting.
///
/// The transceiver remains registered at its index but is no longer counted
/// toward attestation thresholds. If disabling would violate the threshold
/// invariant, the threshold is automatically reduced. Cannot disable the
/// last enabled transceiver to prevent locking the contract.
///
/// # Errors
/// - `TransceiverNotRegistered` if the address is not registered
/// - `TransceiverAlreadyDisabled` if already disabled
/// - `CannotDisableLastTransceiver` if this is the only enabled transceiver
pub fn remove_transceiver(env: &Env, transceiver: &Address) -> Result<(), NttManagerError> {
    let index: u32 =
        get_transceiver_index(env, transceiver).ok_or(NttManagerError::TransceiverNotRegistered)?;

    let mut info: TransceiverInfo = env
        .storage()
        .persistent()
        .get(&DataKey::Transceiver(index))
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    if !info.enabled {
        return Err(NttManagerError::TransceiverAlreadyDisabled);
    }

    let mut bitmap = get_enabled_bitmap(env);
    bitmap.clear(index as u8);

    if bitmap.is_empty() {
        return Err(NttManagerError::CannotDisableLastTransceiver);
    }

    info.enabled = false;
    env.storage()
        .persistent()
        .set(&DataKey::Transceiver(index), &info);
    extend_persistent_ttl(env, &DataKey::Transceiver(index));

    env.storage()
        .instance()
        .set(&DataKey::EnabledBitmap, &bitmap.raw());

    let threshold: u32 = env
        .storage()
        .instance()
        .get(&DataKey::Threshold)
        .unwrap_or(0);
    let enabled_count = bitmap.count_ones() as u32;
    if enabled_count < threshold {
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &enabled_count);
    }

    check_threshold_invariants(env)?;

    Ok(())
}

/// Sets the minimum number of attestations required for inbound transfers.
///
/// The threshold must be at least 1 and cannot exceed the number of enabled
/// transceivers.
///
/// # Errors
/// - `ZeroThreshold` if threshold is 0
/// - `ThresholdTooHigh` if threshold exceeds enabled transceiver count
pub fn set_threshold_value(env: &Env, threshold: u32) -> Result<(), NttManagerError> {
    if threshold == 0 {
        return Err(NttManagerError::ZeroThreshold);
    }

    env.storage()
        .instance()
        .set(&DataKey::Threshold, &threshold);
    check_threshold_invariants(env)?;

    Ok(())
}

/// 64-bit bitmap for tracking transceiver registration and attestations.
///
/// Each bit position corresponds to a transceiver index (0-63). Used for:
/// - Tracking which transceivers are enabled
/// - Recording which transceivers have attested to a message
/// - Computing attestation counts via population count
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[contracttype]
pub struct Bitmap(pub u64);

impl Bitmap {
    /// Creates an empty bitmap with all bits cleared.
    pub fn new() -> Self {
        Self(0)
    }

    /// Sets the bit at the given index.
    ///
    /// # Panics
    /// Panics if `index >= 64`.
    pub fn set(&mut self, index: u8) {
        assert!(index < 64, "bitmap index out of range");
        self.0 |= 1u64 << index;
    }

    /// Clears the bit at the given index.
    ///
    /// # Panics
    /// Panics if `index >= 64`.
    pub fn clear(&mut self, index: u8) {
        assert!(index < 64, "bitmap index out of range");
        self.0 &= !(1u64 << index);
    }

    /// Returns `true` if the bit at the given index is set.
    ///
    /// # Panics
    /// Panics if `index >= 64`.
    pub fn is_set(&self, index: u8) -> bool {
        assert!(index < 64, "bitmap index out of range");
        (self.0 & (1u64 << index)) != 0
    }

    /// Returns the bitwise AND of two bitmaps.
    pub fn and(&self, other: &Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Returns the bitwise OR of two bitmaps.
    pub fn or(&self, other: &Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the number of set bits (population count).
    pub fn count_ones(&self) -> u8 {
        self.0.count_ones() as u8
    }

    /// Returns `true` if no bits are set.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Returns the underlying `u64` value.
    pub fn raw(&self) -> u64 {
        self.0
    }
}
