#![no_std]
extern crate alloc;

use soroban_sdk::{
    address_payload::AddressPayload, contract, contracterror, contractimpl, contracttype,
    panic_with_error, Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec,
};
use stellar_ntt_manager::{AttestationResult, NttManagerError};
use wormhole_interface::{ConsistencyLevel, Error as WormholeError, VAA};

const WH_TRANSCEIVER_PREFIX: [u8; 4] = [0x99, 0x45, 0xff, 0x10];
const PREFIX_LEN: u32 = 4;
const ADDRESS_LEN: u32 = 32;
const LENGTH_PREFIX_LEN: u32 = 2;
const MIN_MESSAGE_LEN: u32 =
    PREFIX_LEN + ADDRESS_LEN + ADDRESS_LEN + LENGTH_PREFIX_LEN + LENGTH_PREFIX_LEN;

const INSTANCE_TTL_THRESHOLD: u32 = 17280;
const INSTANCE_TTL_EXTEND: u32 = 17280 * 30;
const PERSISTENT_TTL_THRESHOLD: u32 = 17280;
const PERSISTENT_TTL_EXTEND: u32 = 17280 * 30;
const VAA_VALIDITY_WINDOW_SECONDS: u64 = 7 * 60 * 60;
const VAA_FUTURE_SKEW_SECONDS: u64 = 60 * 60;

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
    VaaTooOld = 37,
    VaaTooNew = 38,
}

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Initialized,
    Admin,
    Manager,
    ManagerId,
    WormholeCore,
    Peer(u32),
    Consumed(ConsumedKey),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct PeerInfo {
    pub emitter: BytesN<32>,
    pub enabled: bool,
}

#[derive(Clone)]
#[contracttype]
struct ConsumedKey {
    emitter_chain: u32,
    emitter_address: BytesN<32>,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct DecodedMessage {
    source_manager: BytesN<32>,
    recipient_manager: BytesN<32>,
    manager_payload: Bytes,
    _transceiver_payload: Bytes,
}

fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
}

fn extend_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
}

fn is_initialized_internal(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

fn set_initialized_internal(env: &Env) {
    env.storage().instance().set(&DataKey::Initialized, &true);
}

fn get_admin_internal(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

fn set_admin_internal(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

fn get_manager_internal(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Manager)
}

fn set_manager_internal(env: &Env, manager: &Address) {
    env.storage().instance().set(&DataKey::Manager, manager);
}

fn get_manager_id_internal(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&DataKey::ManagerId)
}

fn set_manager_id_internal(env: &Env, manager_id: &BytesN<32>) {
    env.storage()
        .instance()
        .set(&DataKey::ManagerId, manager_id);
}

fn get_wormhole_core_internal(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::WormholeCore)
}

fn set_wormhole_core_internal(env: &Env, core: &Address) {
    env.storage().instance().set(&DataKey::WormholeCore, core);
}

fn get_peer_info_internal(env: &Env, chain_id: u32) -> Option<PeerInfo> {
    let key = DataKey::Peer(chain_id);
    let info: Option<PeerInfo> = env.storage().persistent().get(&key);
    if info.is_some() {
        extend_persistent_ttl(env, &key);
    }
    info
}

fn set_peer_info_internal(env: &Env, chain_id: u32, info: &PeerInfo) {
    let key = DataKey::Peer(chain_id);
    env.storage().persistent().set(&key, info);
    extend_persistent_ttl(env, &key);
}

fn consumed_key(emitter_chain: u32, emitter_address: &BytesN<32>, sequence: u64) -> DataKey {
    DataKey::Consumed(ConsumedKey {
        emitter_chain,
        emitter_address: emitter_address.clone(),
        sequence,
    })
}

fn is_consumed_internal(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) -> bool {
    let key = consumed_key(emitter_chain, emitter_address, sequence);
    let consumed: bool = env.storage().persistent().get(&key).unwrap_or(false);
    if consumed {
        extend_persistent_ttl(env, &key);
    }
    consumed
}

fn set_consumed_internal(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) {
    let key = consumed_key(emitter_chain, emitter_address, sequence);
    env.storage().persistent().set(&key, &true);
    extend_persistent_ttl(env, &key);
}

fn require_initialized(env: &Env) {
    if !is_initialized_internal(env) {
        panic_with_error!(env, TransceiverError::NotInitialized);
    }
}

fn require_admin_auth(env: &Env) -> Address {
    require_initialized(env);
    let admin = get_admin_internal(env)
        .unwrap_or_else(|| panic_with_error!(env, TransceiverError::NotInitialized));
    admin.require_auth();
    admin
}

fn require_manager_auth(env: &Env) -> Address {
    require_initialized(env);
    let manager = get_manager_internal(env)
        .unwrap_or_else(|| panic_with_error!(env, TransceiverError::ManagerNotSet));
    manager.require_auth();
    manager
}

fn address_to_bytes32(_env: &Env, address: &Address) -> BytesN<32> {
    match address.to_payload().expect("address has no payload") {
        AddressPayload::AccountIdPublicKeyEd25519(bytes) => bytes,
        AddressPayload::ContractIdHash(bytes) => bytes,
    }
}

fn read_u16_be(msg: &Bytes, offset: u32) -> Result<u16, TransceiverError> {
    let b0 = msg.get(offset).ok_or(TransceiverError::MessageTooShort)?;
    let b1 = msg
        .get(offset + 1)
        .ok_or(TransceiverError::MessageTooShort)?;
    Ok(u16::from_be_bytes([b0, b1]))
}

fn read_32_bytes(msg: &Bytes, offset: u32) -> Result<[u8; 32], TransceiverError> {
    let mut out = [0u8; 32];
    let mut i = 0u32;
    while i < 32 {
        out[i as usize] = msg
            .get(offset + i)
            .ok_or(TransceiverError::MessageTooShort)?;
        i += 1;
    }
    Ok(out)
}

fn decode_transceiver_message(env: &Env, msg: &Bytes) -> Result<DecodedMessage, TransceiverError> {
    let len = msg.len();
    if len < MIN_MESSAGE_LEN {
        return Err(TransceiverError::MessageTooShort);
    }

    let mut prefix = [0u8; 4];
    for i in 0..4 {
        prefix[i] = msg.get(i as u32).ok_or(TransceiverError::MessageTooShort)?;
    }
    if prefix != WH_TRANSCEIVER_PREFIX {
        return Err(TransceiverError::InvalidTransceiverPrefix);
    }

    let src_arr = read_32_bytes(msg, PREFIX_LEN)?;
    let dst_arr = read_32_bytes(msg, PREFIX_LEN + ADDRESS_LEN)?;

    let mut offset = PREFIX_LEN + ADDRESS_LEN + ADDRESS_LEN;
    let manager_payload_len = read_u16_be(msg, offset)? as u32;
    offset += LENGTH_PREFIX_LEN;

    if offset + manager_payload_len + LENGTH_PREFIX_LEN > len {
        return Err(TransceiverError::MessageTooShort);
    }

    let manager_payload = msg.slice(offset..offset + manager_payload_len);
    offset += manager_payload_len;

    let transceiver_payload_len = read_u16_be(msg, offset)? as u32;
    offset += LENGTH_PREFIX_LEN;

    if offset + transceiver_payload_len > len {
        return Err(TransceiverError::MessageTooShort);
    }

    let transceiver_payload = msg.slice(offset..offset + transceiver_payload_len);

    Ok(DecodedMessage {
        source_manager: BytesN::<32>::from_array(env, &src_arr),
        recipient_manager: BytesN::<32>::from_array(env, &dst_arr),
        manager_payload,
        _transceiver_payload: transceiver_payload,
    })
}

fn encode_transceiver_message(
    env: &Env,
    source_manager: &BytesN<32>,
    recipient_manager: &BytesN<32>,
    manager_payload: &Bytes,
) -> Result<Bytes, TransceiverError> {
    if manager_payload.len() > u16::MAX as u32 {
        return Err(TransceiverError::PayloadTooLong);
    }

    let mut out = Bytes::new(env);
    out.append(&Bytes::from_array(env, &WH_TRANSCEIVER_PREFIX));
    out.append(&Bytes::from_array(env, &source_manager.to_array()));
    out.append(&Bytes::from_array(env, &recipient_manager.to_array()));

    let payload_len = manager_payload.len() as u16;
    out.append(&Bytes::from_array(env, &payload_len.to_be_bytes()));
    out.append(manager_payload);

    out.append(&Bytes::from_array(env, &0u16.to_be_bytes()));

    Ok(out)
}

#[contract]
pub struct TransceiverContract;

fn init_internal(
    env: &Env,
    admin: &Address,
    manager: &Address,
    manager_id: &BytesN<32>,
    wormhole_core: &Address,
) {
    if is_initialized_internal(env) {
        panic_with_error!(env, TransceiverError::AlreadyInitialized);
    }

    let derived_manager_id = address_to_bytes32(env, manager);
    if derived_manager_id != *manager_id {
        panic_with_error!(env, TransceiverError::InvalidManagerId);
    }

    set_admin_internal(env, admin);
    set_manager_internal(env, manager);
    set_manager_id_internal(env, manager_id);
    set_wormhole_core_internal(env, wormhole_core);
    set_initialized_internal(env);
    extend_instance_ttl(env);
}

#[contractimpl]
impl TransceiverContract {
    pub fn __constructor(
        env: Env,
        admin: Address,
        manager: Address,
        manager_id: BytesN<32>,
        wormhole_core: Address,
    ) {
        init_internal(&env, &admin, &manager, &manager_id, &wormhole_core);
    }

    pub fn init(
        env: Env,
        admin: Address,
        manager: Address,
        manager_id: BytesN<32>,
        wormhole_core: Address,
    ) {
        admin.require_auth();
        init_internal(&env, &admin, &manager, &manager_id, &wormhole_core);
    }

    pub fn is_initialized(env: Env) -> bool {
        is_initialized_internal(&env)
    }

    pub fn get_admin(env: Env) -> Address {
        require_initialized(&env);
        get_admin_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::NotInitialized))
    }

    pub fn set_admin(env: Env, new_admin: Address) {
        let _admin = require_admin_auth(&env);
        new_admin.require_auth();
        set_admin_internal(&env, &new_admin);
        extend_instance_ttl(&env);
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let _admin = require_admin_auth(&env);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        extend_instance_ttl(&env);
    }

    pub fn set_manager(env: Env, manager: Address, manager_id: BytesN<32>) {
        let _admin = require_admin_auth(&env);
        let derived_manager_id = address_to_bytes32(&env, &manager);
        if derived_manager_id != manager_id {
            panic_with_error!(&env, TransceiverError::InvalidManagerId);
        }
        set_manager_internal(&env, &manager);
        set_manager_id_internal(&env, &manager_id);
        extend_instance_ttl(&env);
    }

    pub fn get_manager(env: Env) -> Address {
        require_initialized(&env);
        get_manager_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::ManagerNotSet))
    }

    pub fn get_manager_id(env: Env) -> BytesN<32> {
        require_initialized(&env);
        get_manager_id_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::NotInitialized))
    }

    pub fn set_wormhole_core(env: Env, wormhole_core: Address) {
        let _admin = require_admin_auth(&env);
        set_wormhole_core_internal(&env, &wormhole_core);
        extend_instance_ttl(&env);
    }

    pub fn get_wormhole_core(env: Env) -> Address {
        require_initialized(&env);
        get_wormhole_core_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::WormholeCoreNotSet))
    }

    pub fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>) {
        let _admin = require_admin_auth(&env);
        if chain_id == 0 {
            panic_with_error!(&env, TransceiverError::InvalidPeerChainIdZero);
        }
        if emitter.to_array() == [0u8; 32] {
            panic_with_error!(&env, TransceiverError::InvalidPeerZeroAddress);
        }
        if get_peer_info_internal(&env, chain_id).is_some() {
            panic_with_error!(&env, TransceiverError::PeerAlreadySet);
        }
        let info = PeerInfo {
            emitter,
            enabled: true,
        };
        set_peer_info_internal(&env, chain_id, &info);
        extend_instance_ttl(&env);
    }

    pub fn update_peer(env: Env, chain_id: u32, emitter: BytesN<32>) {
        let _admin = require_admin_auth(&env);
        if chain_id == 0 {
            panic_with_error!(&env, TransceiverError::InvalidPeerChainIdZero);
        }
        if emitter.to_array() == [0u8; 32] {
            panic_with_error!(&env, TransceiverError::InvalidPeerZeroAddress);
        }
        let mut info = get_peer_info_internal(&env, chain_id)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::PeerNotFound));
        info.emitter = emitter;
        set_peer_info_internal(&env, chain_id, &info);
        extend_instance_ttl(&env);
    }

    pub fn set_peer_enabled(env: Env, chain_id: u32, enabled: bool) {
        let _admin = require_admin_auth(&env);
        let mut info = get_peer_info_internal(&env, chain_id)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::PeerNotFound));
        info.enabled = enabled;
        set_peer_info_internal(&env, chain_id, &info);
        extend_instance_ttl(&env);
    }

    pub fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>> {
        require_initialized(&env);
        get_peer_info_internal(&env, chain_id).map(|info| info.emitter)
    }

    pub fn get_peer_info(env: Env, chain_id: u32) -> Option<PeerInfo> {
        require_initialized(&env);
        get_peer_info_internal(&env, chain_id)
    }

    pub fn is_peer_enabled(env: Env, chain_id: u32) -> bool {
        require_initialized(&env);
        get_peer_info_internal(&env, chain_id)
            .map(|info| info.enabled)
            .unwrap_or(false)
    }

    pub fn send_message(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    ) {
        require_initialized(&env);
        extend_instance_ttl(&env);

        let _manager = require_manager_auth(&env);

        let peer = get_peer_info_internal(&env, recipient_chain)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::PeerNotFound));
        if !peer.enabled {
            panic_with_error!(&env, TransceiverError::PeerDisabled);
        }

        let source_manager = get_manager_id_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::NotInitialized));

        let payload =
            encode_transceiver_message(&env, &source_manager, &recipient_manager, &manager_payload)
                .unwrap_or_else(|err| panic_with_error!(&env, err));

        let core_addr = get_wormhole_core_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::WormholeCoreNotSet));

        let nonce: u32 = 0;
        let consistency = ConsistencyLevel::Confirmed;

        let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
        args.push_back(nonce.into_val(&env));
        args.push_back(payload.into_val(&env));
        args.push_back(consistency.into_val(&env));

        let res: Result<u64, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "post_message"), args);

        if res.is_err() {
            panic_with_error!(&env, TransceiverError::WormholePostFailed);
        }
    }

    pub fn send(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    ) -> u64 {
        require_initialized(&env);
        extend_instance_ttl(&env);

        let _manager = require_manager_auth(&env);

        let peer = get_peer_info_internal(&env, recipient_chain)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::PeerNotFound));
        if !peer.enabled {
            panic_with_error!(&env, TransceiverError::PeerDisabled);
        }

        let source_manager = get_manager_id_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::NotInitialized));

        let payload =
            encode_transceiver_message(&env, &source_manager, &recipient_manager, &manager_payload)
                .unwrap_or_else(|err| panic_with_error!(&env, err));

        let core_addr = get_wormhole_core_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::WormholeCoreNotSet));

        let nonce: u32 = 0;
        let consistency = ConsistencyLevel::Confirmed;

        let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
        args.push_back(nonce.into_val(&env));
        args.push_back(payload.into_val(&env));
        args.push_back(consistency.into_val(&env));

        let res: Result<u64, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "post_message"), args);

        match res {
            Ok(seq) => seq,
            Err(_) => {
                panic_with_error!(&env, TransceiverError::WormholePostFailed);
            }
        }
    }

    pub fn receive_message(env: Env, vaa_bytes: Bytes) {
        require_initialized(&env);
        extend_instance_ttl(&env);

        let core_addr = get_wormhole_core_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::WormholeCoreNotSet));

        let mut verify_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        verify_args.push_back(vaa_bytes.clone().into_val(&env));
        let verified: Result<bool, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "verify_vaa"), verify_args);

        match verified {
            Ok(true) => {}
            Ok(false) => panic_with_error!(&env, TransceiverError::WormholeVerificationFailed),
            Err(_) => panic_with_error!(&env, TransceiverError::WormholeVerificationFailed),
        }

        let mut parse_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        parse_args.push_back(vaa_bytes.into_val(&env));
        let parsed: Result<VAA, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "parse_vaa"), parse_args);

        let vaa = match parsed {
            Ok(v) => v,
            Err(_) => {
                panic_with_error!(&env, TransceiverError::WormholeParseFailed);
            }
        };

        let now = env.ledger().timestamp();
        let vaa_timestamp = vaa.timestamp as u64;
        let max_timestamp = now.saturating_add(VAA_FUTURE_SKEW_SECONDS);
        if vaa_timestamp > max_timestamp {
            panic_with_error!(&env, TransceiverError::VaaTooNew);
        }
        if now.saturating_sub(vaa_timestamp) > VAA_VALIDITY_WINDOW_SECONDS {
            panic_with_error!(&env, TransceiverError::VaaTooOld);
        }

        let peer = get_peer_info_internal(&env, vaa.emitter_chain)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::PeerNotFound));
        if !peer.enabled {
            panic_with_error!(&env, TransceiverError::PeerDisabled);
        }
        if peer.emitter != vaa.emitter_address {
            panic_with_error!(&env, TransceiverError::UnexpectedEmitter);
        }

        let decoded = decode_transceiver_message(&env, &vaa.payload)
            .unwrap_or_else(|err| panic_with_error!(&env, err));

        let our_manager_id = get_manager_id_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::NotInitialized));
        if decoded.recipient_manager != our_manager_id {
            panic_with_error!(&env, TransceiverError::UnexpectedRecipientManager);
        }

        if is_consumed_internal(&env, vaa.emitter_chain, &vaa.emitter_address, vaa.sequence) {
            panic_with_error!(&env, TransceiverError::ReplayDetected);
        }

        set_consumed_internal(&env, vaa.emitter_chain, &vaa.emitter_address, vaa.sequence);

        let manager = get_manager_internal(&env)
            .unwrap_or_else(|| panic_with_error!(&env, TransceiverError::ManagerNotSet));

        let mut mgr_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        mgr_args.push_back(env.current_contract_address().into_val(&env));
        mgr_args.push_back(vaa.emitter_chain.into_val(&env));
        mgr_args.push_back(decoded.source_manager.into_val(&env));
        mgr_args.push_back(decoded.manager_payload.into_val(&env));

        let res: Result<AttestationResult, NttManagerError> = env.invoke_contract(
            &manager,
            &Symbol::new(&env, "attestation_received"),
            mgr_args,
        );

        if res.is_err() {
            panic_with_error!(&env, TransceiverError::ManagerRejectedMessage);
        }
    }

    pub fn receive_vaa(env: Env, vaa_bytes: Bytes) {
        Self::receive_message(env, vaa_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
    };

    fn manager_id_for(env: &Env, manager: &Address) -> BytesN<32> {
        address_to_bytes32(env, manager)
    }

    #[contract]
    struct DummyManager;

    const KEY_LAST_TRANSCEIVER: &str = "lt";
    const KEY_LAST_SRC_CHAIN: &str = "ls";
    const KEY_LAST_SRC_MANAGER: &str = "lm";
    const KEY_LAST_PAYLOAD: &str = "lp";

    #[contractimpl]
    impl DummyManager {
        pub fn attestation_received(
            env: Env,
            transceiver: Address,
            source_chain: u32,
            source_ntt_manager: BytesN<32>,
            payload: Bytes,
        ) -> Result<AttestationResult, NttManagerError> {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_TRANSCEIVER), &transceiver);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_SRC_CHAIN), &source_chain);
            env.storage().instance().set(
                &Symbol::new(&env, KEY_LAST_SRC_MANAGER),
                &source_ntt_manager,
            );
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_PAYLOAD), &payload);

            Ok(AttestationResult {
                approved: true,
                executed: true,
                queued: false,
            })
        }

        pub fn last_transceiver(env: Env) -> Option<Address> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_TRANSCEIVER))
        }

        pub fn last_src_chain(env: Env) -> Option<u32> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_SRC_CHAIN))
        }

        pub fn last_src_manager(env: Env) -> Option<BytesN<32>> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_SRC_MANAGER))
        }

        pub fn last_payload(env: Env) -> Option<Bytes> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_PAYLOAD))
        }
    }

    #[contract]
    struct DummyWormholeCore;

    const KEY_VERIFY_OK: &str = "vok";
    const KEY_VERIFY_ERR: &str = "ver";
    const KEY_PARSE_CHAIN: &str = "pc";
    const KEY_PARSE_EMITTER: &str = "pe";
    const KEY_PARSE_SEQ: &str = "ps";
    const KEY_PARSE_PAYLOAD: &str = "pp";
    const KEY_PARSE_TIMESTAMP: &str = "pt";

    const KEY_LAST_POSTED_PAYLOAD: &str = "ppayload";
    #[contractimpl]
    impl DummyWormholeCore {
        pub fn post_message(
            env: Env,
            _nonce: u32,
            payload: Bytes,
            _consistency_level: ConsistencyLevel,
        ) -> Result<u64, WormholeError> {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_POSTED_PAYLOAD), &payload);
            Ok(42)
        }

        pub fn last_posted_payload(env: Env) -> Option<Bytes> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_POSTED_PAYLOAD))
        }

        pub fn set_verify_result(env: Env, ok: bool, should_error: bool) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_VERIFY_OK), &ok);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_VERIFY_ERR), &should_error);
        }

        pub fn verify_vaa(env: Env, _vaa_bytes: Bytes) -> Result<bool, WormholeError> {
            let should_error: bool = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_VERIFY_ERR))
                .unwrap_or(false);
            if should_error {
                return Err(WormholeError::InvalidVAAFormat);
            }
            Ok(env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_VERIFY_OK))
                .unwrap_or(true))
        }

        pub fn set_parsed_vaa(
            env: Env,
            chain: u32,
            emitter: BytesN<32>,
            sequence: u64,
            timestamp: u32,
            payload: Bytes,
        ) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_PARSE_CHAIN), &chain);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_PARSE_EMITTER), &emitter);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_PARSE_SEQ), &sequence);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_PARSE_TIMESTAMP), &timestamp);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_PARSE_PAYLOAD), &payload);
        }

        pub fn parse_vaa(env: Env, _vaa_bytes: Bytes) -> Result<VAA, WormholeError> {
            let emitter_chain: u32 = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_PARSE_CHAIN))
                .unwrap_or_else(|| panic!("parsed vaa chain not set"));
            let emitter_address: BytesN<32> = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_PARSE_EMITTER))
                .unwrap_or_else(|| panic!("parsed vaa emitter not set"));
            let sequence: u64 = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_PARSE_SEQ))
                .unwrap_or_else(|| panic!("parsed vaa sequence not set"));
            let timestamp: u32 = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_PARSE_TIMESTAMP))
                .unwrap_or_else(|| panic!("parsed vaa timestamp not set"));
            let payload: Bytes = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, KEY_PARSE_PAYLOAD))
                .unwrap_or_else(|| panic!("parsed vaa payload not set"));

            Ok(VAA {
                emitter_chain,
                emitter_address,
                payload,
                nonce: 0,
                sequence,
                consistency_level: 0,
                guardian_set_index: 0,
                timestamp,
                signatures: Vec::new(&env),
                version: 1,
            })
        }
    }

    fn setup_transceiver(env: &Env) -> (TransceiverContractClient<'_>, Address, Address) {
        let admin = Address::generate(env);
        let manager = Address::generate(env);
        let manager_id = manager_id_for(env, &manager);
        let wormhole_core = Address::generate(env);

        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager, &manager_id, &wormhole_core),
        );
        let transceiver = TransceiverContractClient::new(env, &transceiver_id);

        (transceiver, manager, wormhole_core)
    }

    #[test]
    fn send_message_posts_payload() {
        let env = Env::default();
        env.mock_all_auths();

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let manager = Address::generate(&env);
        let manager_id = manager_id_for(&env, &manager);

        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let dst_chain: u32 = 2;
        let peer = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&dst_chain, &peer);

        let recipient_manager = BytesN::<32>::from_array(&env, &[8u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[1u8, 2, 3]);

        transceiver.send_message(&dst_chain, &recipient_manager, &manager_payload);

        let expected =
            encode_transceiver_message(&env, &manager_id, &recipient_manager, &manager_payload)
                .expect("encode should succeed");
        let posted = core_client
            .last_posted_payload()
            .expect("payload should be posted");
        assert_eq!(posted, expected);
    }

    #[test]
    fn receive_message_happy_path_forwards_attestation() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);

        let source_manager = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[1u8, 2, 3, 4]);
        let tm_payload =
            encode_transceiver_message(&env, &source_manager, &manager_id, &manager_payload)
                .expect("encode should succeed");

        let seq: u64 = 1;
        let timestamp = env.ledger().timestamp() as u32;
        core_client.set_parsed_vaa(
            &emitter_chain,
            &emitter_address,
            &seq,
            &timestamp,
            &tm_payload,
        );

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);

        let mgr = DummyManagerClient::new(&env, &manager_contract_id);
        assert_eq!(mgr.last_src_chain().unwrap(), emitter_chain);
        assert_eq!(mgr.last_src_manager().unwrap(), source_manager);
        assert_eq!(mgr.last_payload().unwrap(), manager_payload);
        assert_eq!(mgr.last_transceiver().unwrap(), transceiver_id);
    }

    #[test]
    fn receive_message_panics_on_replay() {
        use core::panic::AssertUnwindSafe;
        use soroban_sdk::testutils::arbitrary::std::panic::catch_unwind;

        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);

        let source_manager = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[9u8]);
        let tm_payload =
            encode_transceiver_message(&env, &source_manager, &manager_id, &manager_payload)
                .expect("encode should succeed");

        let seq: u64 = 1;
        let timestamp = env.ledger().timestamp() as u32;
        core_client.set_parsed_vaa(
            &emitter_chain,
            &emitter_address,
            &seq,
            &timestamp,
            &tm_payload,
        );

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);

        let res = catch_unwind(AssertUnwindSafe(|| {
            transceiver.receive_message(&vaa_bytes);
        }));
        assert!(res.is_err());
    }

    #[test]
    #[should_panic]
    fn receive_message_panics_on_future_timestamp() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);

        let source_manager = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[1u8, 2, 3, 4]);
        let tm_payload =
            encode_transceiver_message(&env, &source_manager, &manager_id, &manager_payload)
                .expect("encode should succeed");

        let seq: u64 = 1;
        let timestamp = env
            .ledger()
            .timestamp()
            .saturating_add(VAA_FUTURE_SKEW_SECONDS + 1) as u32;
        core_client.set_parsed_vaa(
            &emitter_chain,
            &emitter_address,
            &seq,
            &timestamp,
            &tm_payload,
        );

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);
    }

    #[test]
    #[should_panic]
    fn receive_message_panics_on_peer_mismatch() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let emitter_chain: u32 = 2;
        let expected_emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &expected_emitter);

        let wrong_emitter = BytesN::<32>::from_array(&env, &[8u8; 32]);
        let payload = Bytes::from_array(&env, &[1u8]);
        let seq: u64 = 1;
        let timestamp = env.ledger().timestamp() as u32;
        core_client.set_parsed_vaa(&emitter_chain, &wrong_emitter, &seq, &timestamp, &payload);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);
    }

    #[test]
    #[should_panic]
    fn receive_message_panics_on_failed_verification() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);
        core_client.set_verify_result(&false, &false);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);
    }

    #[test]
    #[should_panic]
    fn receive_message_panics_on_disabled_peer() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();
        let manager_id = manager_id_for(&env, &manager_addr);

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let admin = Address::generate(&env);
        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager_addr, &manager_id, &core_id),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);
        transceiver.set_peer_enabled(&emitter_chain, &false);

        let source_manager = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[1u8]);
        let tm_payload =
            encode_transceiver_message(&env, &source_manager, &manager_id, &manager_payload)
                .expect("encode should succeed");

        let seq: u64 = 1;
        let timestamp = env.ledger().timestamp() as u32;
        core_client.set_parsed_vaa(
            &emitter_chain,
            &emitter_address,
            &seq,
            &timestamp,
            &tm_payload,
        );

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_message(&vaa_bytes);
    }

    #[test]
    #[should_panic]
    fn send_message_requires_manager_auth() {
        let env = Env::default();

        let admin = Address::generate(&env);
        let manager = Address::generate(&env);
        let manager_id = manager_id_for(&env, &manager);
        let core = Address::generate(&env);

        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager, &manager_id, &core),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let dst_chain: u32 = 2;
        let peer = BytesN::<32>::from_array(&env, &[7u8; 32]);
        env.as_contract(&transceiver_id, || {
            set_peer_info_internal(
                &env,
                dst_chain,
                &PeerInfo {
                    emitter: peer.clone(),
                    enabled: true,
                },
            );
        });

        let recipient_manager = BytesN::<32>::from_array(&env, &[8u8; 32]);
        let manager_payload = Bytes::from_array(&env, &[1u8]);
        transceiver.send_message(&dst_chain, &recipient_manager, &manager_payload);
    }

    #[test]
    #[should_panic]
    fn set_peer_requires_admin_auth() {
        let env = Env::default();

        let admin = Address::generate(&env);
        let manager = Address::generate(&env);
        let manager_id = manager_id_for(&env, &manager);
        let core = Address::generate(&env);

        let transceiver_id = env.register(
            TransceiverContract,
            TransceiverContractArgs::__constructor(&admin, &manager, &manager_id, &core),
        );
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let chain_id: u32 = 2;
        let emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&chain_id, &emitter);
    }
}
