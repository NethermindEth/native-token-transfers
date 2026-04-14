use soroban_sdk::{contractclient, contracterror, contracttype, Address, Bytes, BytesN, Env};

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct PeerInfo {
    pub emitter: BytesN<32>,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[contracterror]
#[repr(u32)]
pub enum TransceiverError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidManagerId = 4,
    ManagerNotSet = 5,
    WormholeCoreNotSet = 6,
    InvalidPeerChainIdZero = 10,
    InvalidPeerZeroAddress = 11,
    PeerAlreadySet = 12,
    PeerNotFound = 13,
    PeerDisabled = 14,
    ChainIdTooLarge = 15,
    WormholeVerificationFailed = 20,
    WormholeParseFailed = 21,
    WormholePostFailed = 22,
    InvalidTransceiverPrefix = 30,
    MessageTooShort = 31,
    PayloadTooLong = 32,
    UnexpectedRecipientManager = 33,
    ReplayDetected = 34,
    UnexpectedEmitter = 35,
    ManagerRejectedMessage = 36,
}

#[contractclient(name = "WormholeTransceiverClient")]
pub trait WormholeTransceiverInterface {
    fn get_admin(env: Env) -> Address;
    fn set_admin(env: Env, new_admin: Address);
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
    fn get_wormhole_core(env: Env) -> Address;
    fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>);
    fn update_peer(env: Env, chain_id: u32, emitter: BytesN<32>);
    fn set_peer_enabled(env: Env, chain_id: u32, enabled: bool);
    fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>>;
    fn get_peer_info(env: Env, chain_id: u32) -> Option<PeerInfo>;
    fn is_peer_enabled(env: Env, chain_id: u32) -> bool;
    fn receive_message(env: Env, vaa_bytes: Bytes);
    fn receive_vaa(env: Env, vaa_bytes: Bytes);
}
