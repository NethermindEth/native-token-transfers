use soroban_sdk::{contracttype, Address};

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TransceiverInfo {
    pub address: Address,
    pub enabled: bool,
    pub index: u32,
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


