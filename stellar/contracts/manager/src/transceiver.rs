use soroban_sdk::{contracttype, Address};

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransceiverInfo {
    pub address: Address,
    pub enabled: bool,
    pub index: u32,
}

pub fn get_enabled_bitmap(env: &Env) -> Bitmap {
    let raw: u64 = env
        .storage()
        .instance()
        .get(&DataKey::EnabledBitmap)
        .unwrap_or(0);
    Bitmap(raw)
}

pub fn get_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::Threshold)
        .unwrap_or(0)
}

pub fn get_transceiver(env: &Env, index: u32) -> Option<TransceiverInfo> {
    env.storage().persistent().get(&DataKey::Transceiver(index))
}

pub fn get_transceiver_index(env: &Env, address: &Address) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&DataKey::TransceiverIndex(address.clone()))
}

pub fn is_transceiver_enabled(env: &Env, address: &Address) -> bool {
    if let Some(index) = get_transceiver_index(env, address) {
        if let Some(info) = get_transceiver(env, index) {
            return info.enabled;
        }
    }
    false
}

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

fn check_threshold_invariants(env: &Env) -> Result<(), NttManagerError> {
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

pub fn set_transceiver(env: &Env, transceiver: Address) -> Result<u32, NttManagerError> {
    let existing_index = get_transceiver_index(env, &transceiver);

    if let Some(index) = existing_index {
        let mut info: TransceiverInfo = env
            .storage()
            .persistent()
            .get(&DataKey::Transceiver(index))
            .ok_or(NttManagerError::TransceiverNotRegistered)?;

        if !info.enabled {
            info.enabled = true;
            env.storage()
                .persistent()
                .set(&DataKey::Transceiver(index), &info);

            let mut bitmap = get_enabled_bitmap(env);
            bitmap.set(index as u8);
            env.storage()
                .instance()
                .set(&DataKey::EnabledBitmap, &bitmap.raw());
        }

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
    env.storage()
        .persistent()
        .set(&DataKey::TransceiverIndex(transceiver), &index);
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

pub fn remove_transceiver(env: &Env, transceiver: &Address) -> Result<(), NttManagerError> {
    let index: u32 = get_transceiver_index(env, transceiver)
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    let mut info: TransceiverInfo = env
        .storage()
        .persistent()
        .get(&DataKey::Transceiver(index))
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    if !info.enabled {
        return Ok(());
    }

    info.enabled = false;
    env.storage()
        .persistent()
        .set(&DataKey::Transceiver(index), &info);

    let mut bitmap = get_enabled_bitmap(env);
    bitmap.clear(index as u8);
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

pub fn set_threshold_value(env: &Env, threshold: u32) -> Result<(), NttManagerError> {
    if threshold == 0 {
        return Err(NttManagerError::ZeroThreshold);
    }

    env.storage().instance().set(&DataKey::Threshold, &threshold);
    check_threshold_invariants(env)?;

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[contracttype]
pub struct Bitmap(pub u64);

impl Bitmap {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn set(&mut self, index: u8) {
        assert!(index < 64, "bitmap index out of range");
        self.0 |= 1u64 << index;
    }

    pub fn clear(&mut self, index: u8) {
        assert!(index < 64, "bitmap index out of range");
        self.0 &= !(1u64 << index);
    }

    pub fn is_set(&self, index: u8) -> bool {
        assert!(index < 64, "bitmap index out of range");
        (self.0 & (1u64 << index)) != 0
    }

    pub fn and(&self, other: &Self) -> Self {
        Self(self.0 & other.0)
    }

    pub fn or(&self, other: &Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn count_ones(&self) -> u8 {
        self.0.count_ones() as u8
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn raw(&self) -> u64 {
        self.0
    }
}


