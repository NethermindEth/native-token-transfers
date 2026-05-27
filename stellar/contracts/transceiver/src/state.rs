use soroban_sdk::{contracttype, BytesN};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Manager,
    WormholeCore,
    Peer(u32),
    Consumed(ConsumedKey),
    Version,
}

#[derive(Clone)]
#[contracttype]
pub struct ConsumedKey {
    pub emitter_chain: u32,
    pub emitter_address: BytesN<32>,
    pub sequence: u64,
}
