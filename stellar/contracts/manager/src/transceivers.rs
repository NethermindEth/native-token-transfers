//! Transceiver registry for managing cross-chain message relayers.
//!
//! Transceivers are responsible for sending and receiving messages across chains.
//! This module provides a bitmap-based registry that tracks up to 64 transceivers,
//! along with threshold-based attestation requirements.
use soroban_ntt_client::{NttManagerError, MAX_TRANSCEIVERS};
use soroban_sdk::{contracttype, Address, Env, Vec};

use crate::storage::{InstanceStorage, TransceiverEntry, TransceiverIndexEntry};

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
    /// Returns `BitmapIndexOutOfRange` if `index >= 64`.
    pub fn set(&mut self, index: u32) -> Result<(), NttManagerError> {
        if index >= 64 {
            return Err(NttManagerError::BitmapIndexOutOfRange);
        }
        self.0 |= 1u64 << index;
        Ok(())
    }

    /// Clears the bit at the given index.
    ///
    /// Returns `BitmapIndexOutOfRange` if `index >= 64`.
    pub fn clear(&mut self, index: u32) -> Result<(), NttManagerError> {
        if index >= 64 {
            return Err(NttManagerError::BitmapIndexOutOfRange);
        }
        self.0 &= !(1u64 << index);
        Ok(())
    }

    /// Returns `true` if the bit at the given index is set.
    ///
    /// Returns `BitmapIndexOutOfRange` if `index >= 64`.
    pub fn is_set(&self, index: u32) -> Result<bool, NttManagerError> {
        if index >= 64 {
            return Err(NttManagerError::BitmapIndexOutOfRange);
        }
        Ok((self.0 & (1u64 << index)) != 0)
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
    Bitmap(InstanceStorage::new(env).enabled_bitmap())
}

/// Retrieves the current attestation threshold.
///
/// Returns 0 if no transceivers have been registered.
pub fn get_threshold(env: &Env) -> u32 {
    InstanceStorage::new(env).threshold()
}

/// Retrieves transceiver info by its permanent index.
///
/// Returns `None` if no transceiver exists at the given index.
pub fn get_transceiver(env: &Env, index: u32) -> Option<TransceiverInfo> {
    TransceiverEntry::new(env, index).get()
}

/// Looks up a transceiver's index by its contract address.
///
/// Returns `None` if the address is not registered.
pub fn get_transceiver_index(env: &Env, address: &Address) -> Option<u32> {
    TransceiverIndexEntry::new(env, address.clone()).get()
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
pub fn get_enabled_transceivers(env: &Env) -> Result<Vec<Address>, NttManagerError> {
    let storage = InstanceStorage::new(env);
    let bitmap = Bitmap(storage.enabled_bitmap());
    let count = storage.transceiver_count();

    let mut result = Vec::new(env);
    for i in 0..count {
        if bitmap.is_set(i)? {
            if let Some(info) = get_transceiver(env, i) {
                result.push_back(info.address);
            }
        }
    }
    Ok(result)
}

/// Validates threshold invariants after registry modifications.
///
/// Enforces:
/// - INV-023: `threshold <= enabled_count`
/// - INV-024: `threshold > 0` when transceivers exist
pub fn check_threshold_invariants(env: &Env) -> Result<(), NttManagerError> {
    let storage = InstanceStorage::new(env);
    let threshold = storage.threshold();
    let enabled_count = Bitmap(storage.enabled_bitmap()).count_ones() as u32;

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
    let storage = InstanceStorage::new(env);
    let existing_index = get_transceiver_index(env, &transceiver);

    if let Some(index) = existing_index {
        let entry = TransceiverEntry::new(env, index);
        let mut info = entry
            .get()
            .ok_or(NttManagerError::TransceiverNotRegistered)?;

        if info.enabled {
            return Err(NttManagerError::TransceiverAlreadyEnabled);
        }

        info.enabled = true;
        entry.set(&info);

        let mut bitmap = Bitmap(storage.enabled_bitmap());
        bitmap.set(index)?;
        storage.set_enabled_bitmap(bitmap.raw());

        check_threshold_invariants(env)?;
        return Ok(index);
    }

    let count = storage.transceiver_count();

    if count >= MAX_TRANSCEIVERS {
        return Err(NttManagerError::MaxTransceiversReached);
    }

    let index = count;
    let info = TransceiverInfo {
        address: transceiver.clone(),
        enabled: true,
        index,
    };

    TransceiverEntry::new(env, index).set(&info);
    TransceiverIndexEntry::new(env, transceiver).set(&index);
    storage.set_transceiver_count(count + 1);

    let mut bitmap = Bitmap(storage.enabled_bitmap());
    bitmap.set(index)?;
    storage.set_enabled_bitmap(bitmap.raw());

    let threshold = storage.threshold();
    if threshold == 0 {
        storage.set_threshold(1);
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
    let storage = InstanceStorage::new(env);

    let index =
        get_transceiver_index(env, transceiver).ok_or(NttManagerError::TransceiverNotRegistered)?;

    let entry = TransceiverEntry::new(env, index);
    let mut info = entry
        .get()
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    if !info.enabled {
        return Err(NttManagerError::TransceiverAlreadyDisabled);
    }

    let mut bitmap = Bitmap(storage.enabled_bitmap());
    bitmap.clear(index)?;

    if bitmap.is_empty() {
        return Err(NttManagerError::CannotDisableLastTransceiver);
    }

    info.enabled = false;
    entry.set(&info);

    storage.set_enabled_bitmap(bitmap.raw());

    let threshold = storage.threshold();
    let enabled_count = bitmap.count_ones() as u32;
    if enabled_count < threshold {
        storage.set_threshold(enabled_count);
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

    let storage = InstanceStorage::new(env);
    storage.set_threshold(threshold);
    check_threshold_invariants(env)?;

    Ok(())
}
