use soroban_sdk::{contracttype, BytesN};

pub(crate) const TRANSCEIVER_TYPE: [u8; 8] = *b"wormhole";

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Initialized,
    Admin,
    Manager,
    ManagerId,
    WormholeCore,
    Peer(u32),
    Consumed(ConsumedKey),
}

#[derive(Clone)]
#[contracttype]
pub struct ConsumedKey {
    pub emitter_chain: u32,
    pub emitter_address: BytesN<32>,
    pub sequence: u64,
}
