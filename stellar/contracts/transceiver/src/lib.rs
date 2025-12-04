#![no_std]
extern crate alloc;

use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec};
use wormhole_interface::{ConsistencyLevel, Error as WormholeError, VAA};

const NTT_PREFIX: u32 = 0x994E5454;
const TM_PREFIX_LEN: u32 = 4;
const TM_MANAGER_ID_LEN: u32 = 32;
const TM_HEADER_LEN: u32 = TM_PREFIX_LEN + TM_MANAGER_ID_LEN + TM_MANAGER_ID_LEN;

fn encode_transceiver_message(
    env: &Env,
    source_manager: &BytesN<32>,
    recipient_manager: &BytesN<32>,
    manager_payload: &Bytes,
) -> Bytes {
    let mut out = Bytes::new(env);

    // Layout is: prefix || source_manager || recipient_manager || manager_payload
    out.append(&Bytes::from_array(env, &NTT_PREFIX.to_be_bytes()));
    out.append(&Bytes::from_array(env, &source_manager.to_array()));
    out.append(&Bytes::from_array(env, &recipient_manager.to_array()));
    out.append(manager_payload);

    out
}

fn read_u32_be(msg: &Bytes, offset: u32) -> u32 {
    // TODO: Move to other repo if it makes sense
    let b0 = msg
        .get(offset)
        .unwrap_or_else(|| panic!("message too short"));
    let b1 = msg
        .get(offset + 1)
        .unwrap_or_else(|| panic!("message too short"));
    let b2 = msg
        .get(offset + 2)
        .unwrap_or_else(|| panic!("message too short"));
    let b3 = msg
        .get(offset + 3)
        .unwrap_or_else(|| panic!("message too short"));
    u32::from_be_bytes([b0, b1, b2, b3])
}

fn read_32_bytes(msg: &Bytes, offset: u32) -> [u8; 32] {
    // TODO: Move to other repo if it makes sense
    let mut out = [0u8; 32];
    let mut i = 0u32;
    while i < 32 {
        out[i as usize] = msg
            .get(offset + i)
            .unwrap_or_else(|| panic!("message too short"));
        i += 1;
    }
    out
}

fn decode_transceiver_message(env: &Env, msg: &Bytes) -> (BytesN<32>, BytesN<32>, Bytes) {
    let len = msg.len();
    if len < TM_HEADER_LEN {
        panic!("transceiver message too short");
    }

    let prefix = read_u32_be(msg, 0);
    if prefix != NTT_PREFIX {
        panic!("invalid transceiver prefix");
    }

    let src_arr = read_32_bytes(msg, TM_PREFIX_LEN);
    let dst_arr = read_32_bytes(msg, TM_PREFIX_LEN + TM_MANAGER_ID_LEN);

    let src = BytesN::<32>::from_array(env, &src_arr);
    let dst = BytesN::<32>::from_array(env, &dst_arr);

    // Everything after the fixed header is treated as payload.
    let payload = msg.slice(TM_HEADER_LEN..len);

    (src, dst, payload)
}

fn require_initialized(env: &Env) {
    if !is_initialized_internal(env) {
        panic!("not initialized");
    }
}

fn require_manager_auth(env: &Env) {
    require_initialized(env);
    let manager = get_manager_internal(env).unwrap_or_else(|| panic!("manager not set"));
    manager.require_auth();
}

#[contract]
pub struct TransceiverContract;

#[contractimpl]
impl TransceiverContract {
    pub fn init(env: Env, manager: Address, manager_id: BytesN<32>, wormhole_core: Address) {
        if is_initialized_internal(&env) {
            panic!("already initialized");
        }

        manager.require_auth();

        set_manager_internal(&env, &manager);
        set_manager_id_internal(&env, &manager_id);
        set_wormhole_core_internal(&env, &wormhole_core);

        set_initialized_internal(&env);

        // TODO How are upgrades handled?
        // - who can upgrade?
        // - how to keep replay protection accross upgrade?
    }

    pub fn is_initialized(env: Env) -> bool {
        is_initialized_internal(&env)
    }

    pub fn set_manager(env: Env, manager: Address) {
        require_manager_auth(&env);
        set_manager_internal(&env, &manager);
    }

    pub fn get_manager(env: Env) -> Address {
        require_initialized(&env);
        get_manager_internal(&env).unwrap_or_else(|| panic!("manager not set"))
    }

    pub fn set_wormhole_core(env: Env, wormhole_core: Address) {
        require_manager_auth(&env);
        set_wormhole_core_internal(&env, &wormhole_core);
    }

    pub fn get_wormhole_core(env: Env) -> Address {
        require_initialized(&env);
        get_wormhole_core_internal(&env).unwrap_or_else(|| panic!("wormhole core not set"))
    }

    pub fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>) {
        require_manager_auth(&env);
        set_peer_internal(&env, chain_id, &emitter);

        // TODO: Other chains: we need a list of all peers, otherwise all VAAs will be rejected.
    }

    pub fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>> {
        require_initialized(&env);
        get_peer_internal(&env, chain_id)
    }

    pub fn send(
        env: Env,
        _dst_chain_id: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    ) -> u64 {
        require_initialized(&env);

        let core_addr =
            get_wormhole_core_internal(&env).unwrap_or_else(|| panic!("wormhole core not set"));

        let manager = get_manager_internal(&env).unwrap_or_else(|| panic!("manager not set"));
        manager.require_auth();

        let source_manager =
            get_manager_id_internal(&env).unwrap_or_else(|| panic!("manager id not set"));

        let payload =
            encode_transceiver_message(&env, &source_manager, &recipient_manager, &manager_payload);

        let emitter = env.current_contract_address();
        let nonce: u32 = 0;
        let consistency = ConsistencyLevel::Confirmed;

        let mut args: Vec<soroban_sdk::Val> = Vec::new(&env);
        args.push_back(emitter.into_val(&env));
        args.push_back(nonce.into_val(&env));
        args.push_back(payload.into_val(&env));
        args.push_back(consistency.into_val(&env));

        let res: Result<u64, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "post_message"), args);

        match res {
            Ok(seq) => seq,
            Err(_e) => panic!("post_message failed"),
        }
    }

    pub fn receive_vaa(env: Env, vaa_bytes: Bytes) {
        require_initialized(&env);

        let core_addr =
            get_wormhole_core_internal(&env).unwrap_or_else(|| panic!("wormhole core not set"));

        let mut verify_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        verify_args.push_back(vaa_bytes.into_val(&env));

        let verified: Result<bool, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "verify_vaa"), verify_args);

        match verified {
            Ok(true) => {}
            Ok(false) => panic!("VAA failed verification"),
            Err(_e) => panic!("verify_vaa failed"),
        }

        let mut parse_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        parse_args.push_back(vaa_bytes.into_val(&env));

        let parsed: Result<VAA, WormholeError> =
            env.invoke_contract(&core_addr, &Symbol::new(&env, "parse_vaa"), parse_args);

        let vaa = match parsed {
            Ok(v) => v,
            Err(_e) => panic!("parse_vaa failed"),
        };

        let emitter_chain = vaa.emitter_chain;
        let emitter_address = vaa.emitter_address;
        let sequence = vaa.sequence;

        // Replay protection key: (emitter_chain, emitter_address, sequence).
        if is_consumed_internal(&env, emitter_chain, &emitter_address, sequence) {
            panic!("vaa already consumed");
        }

        // TODO: Decide whether to set-consumed here or only after all validation passes.
        // Currently, a VAA is marked consumed even if later checks fail (peer mismatch, bad payload,
        // wrong recipient_manager). That can be desirable (DoS resistance) or undesirable (burns messages).
        set_consumed_internal(&env, emitter_chain, &emitter_address, sequence);

        // TODO: Handling retention

        let expected_peer = get_peer_internal(&env, emitter_chain)
            .unwrap_or_else(|| panic!("no peer configured for emitter chain"));

        if emitter_address != expected_peer {
            panic!("unexpected emitter for chain");
        }

        // VAA payload must be in NTT transceiver message format
        let (_source_manager, recipient_manager, manager_payload) =
            decode_transceiver_message(&env, &vaa.payload);

        let our_manager_id =
            get_manager_id_internal(&env).unwrap_or_else(|| panic!("manager id not set"));

        if recipient_manager != our_manager_id {
            panic!("transceiver message not intended for this manager");
        }

        // TODO Should we validate source manager?
        let manager = get_manager_internal(&env).unwrap_or_else(|| panic!("manager not set"));

        let mut mgr_args: Vec<soroban_sdk::Val> = Vec::new(&env);
        mgr_args.push_back(emitter_chain.into_val(&env));
        mgr_args.push_back(manager_payload.into_val(&env));

        let _: () = env.invoke_contract(&manager, &Symbol::new(&env, "receive_message"), mgr_args);
    }

    pub fn get_manager_id(env: Env) -> BytesN<32> {
        require_initialized(&env);
        get_manager_id_internal(&env).unwrap_or_else(|| panic!("manager id not set"))
    }
}

const KEY_INIT: &str = "i";
const KEY_MANAGER: &str = "m";
const KEY_WORMHOLE_CORE: &str = "w";
const KEY_PEER: &str = "p";
const KEY_MANAGER_ID: &str = "mi";
const KEY_CONSUMED: &str = "c";

fn init_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_INIT)
}

fn manager_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_MANAGER)
}

fn wormhole_core_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_WORMHOLE_CORE)
}

fn peer_key(env: &Env, chain_id: u32) -> (Symbol, u32) {
    (Symbol::new(env, KEY_PEER), chain_id)
}

fn manager_id_key(env: &Env) -> Symbol {
    Symbol::new(env, KEY_MANAGER_ID)
}

fn set_initialized_internal(env: &Env) {
    let key = init_key(env);
    env.storage().instance().set(&key, &true);
}

fn is_initialized_internal(env: &Env) -> bool {
    let key = init_key(env);
    env.storage().instance().get(&key).unwrap_or(false)
}

fn set_manager_internal(env: &Env, manager: &Address) {
    let key = manager_key(env);
    env.storage().instance().set(&key, manager);
}

fn get_manager_internal(env: &Env) -> Option<Address> {
    let key = manager_key(env);
    env.storage().instance().get(&key)
}

fn set_wormhole_core_internal(env: &Env, wormhole_core: &Address) {
    let key = wormhole_core_key(env);
    env.storage().instance().set(&key, wormhole_core);
}

fn get_wormhole_core_internal(env: &Env) -> Option<Address> {
    let key = wormhole_core_key(env);
    env.storage().instance().get(&key)
}

fn set_peer_internal(env: &Env, chain_id: u32, emitter: &BytesN<32>) {
    let key = peer_key(env, chain_id);
    env.storage().instance().set(&key, emitter);
}

fn get_peer_internal(env: &Env, chain_id: u32) -> Option<BytesN<32>> {
    let key = peer_key(env, chain_id);
    env.storage().instance().get(&key)
}

fn set_manager_id_internal(env: &Env, manager_id: &BytesN<32>) {
    let key = manager_id_key(env);
    env.storage().instance().set(&key, manager_id);
}

fn get_manager_id_internal(env: &Env) -> Option<BytesN<32>> {
    let key = manager_id_key(env);
    env.storage().instance().get(&key)
}

fn consumed_key(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) -> (Symbol, u32, BytesN<32>, u64) {
    (
        Symbol::new(env, KEY_CONSUMED),
        emitter_chain,
        emitter_address.clone(),
        sequence,
    )
}

fn is_consumed_internal(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) -> bool {
    let key = consumed_key(env, emitter_chain, emitter_address, sequence);
    env.storage().instance().get(&key).unwrap_or(false)
}

fn set_consumed_internal(
    env: &Env,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) {
    let key = consumed_key(env, emitter_chain, emitter_address, sequence);
    env.storage().instance().set(&key, &true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use soroban_sdk::testutils::arbitrary::std::panic::catch_unwind;
    use soroban_sdk::{
        contract, contractimpl, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
    };
    use wormhole_interface::{ConsistencyLevel, Error as WormholeError, VAA};

    fn setup() -> (Env, BytesN<32>, BytesN<32>) {
        let env = Env::default();
        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        let recipient_manager = BytesN::<32>::from_array(&env, &[8u8; 32]);
        (env, manager_id, recipient_manager)
    }

    fn auth_invoke(
        env: &Env,
        _addr: &Address,
        _contract_id: &Address,
        _fn_name: &'static str,
        _args: Vec<soroban_sdk::Val>,
    ) {
        env.mock_all_auths();
    }

    #[contract]
    pub struct DummyManager;

    const KEY_LAST_SRC: &str = "ls";
    const KEY_LAST_MSG: &str = "lm";
    const KEY_PARSE_SEQ: &str = "ps";

    #[contractimpl]
    impl DummyManager {
        pub fn receive_message(env: Env, src_chain_id: u32, message: Bytes) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_SRC), &src_chain_id);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, KEY_LAST_MSG), &message);
        }

        pub fn last_src_chain(env: Env) -> Option<u32> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_SRC))
        }

        pub fn last_message(env: Env) -> Option<Bytes> {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, KEY_LAST_MSG))
        }
    }

    #[contract]
    pub struct DummyWormholeCore;

    const KEY_PARSE_CHAIN: &str = "pc";
    const KEY_PARSE_EMITTER: &str = "pe";
    const KEY_PARSE_PAYLOAD: &str = "pp";

    #[contractimpl]
    impl DummyWormholeCore {
        pub fn post_message(
            _env: Env,
            _emitter: Address,
            _nonce: u32,
            _payload: Bytes,
            _consistency_level: ConsistencyLevel,
        ) -> Result<u64, WormholeError> {
            Ok(42)
        }

        pub fn verify_vaa(_env: Env, _vaa_bytes: Bytes) -> Result<bool, WormholeError> {
            Ok(true)
        }

        pub fn set_parsed_vaa(
            env: Env,
            chain: u32,
            emitter: BytesN<32>,
            sequence: u64,
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
                timestamp: 0,
                signatures: Vec::new(&env),
                version: 1,
            })
        }
    }

    #[test]
    fn init_is_one_shot() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(TransceiverContract, ());
        let client = TransceiverContractClient::new(&env, &contract_id);

        let manager = Address::generate(&env);
        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        let wormhole_core = Address::generate(&env);

        client.init(&manager, &manager_id, &wormhole_core);

        let res = catch_unwind(core::panic::AssertUnwindSafe(|| {
            client.init(&manager, &manager_id, &wormhole_core);
        }));
        assert!(res.is_err());
    }

    #[test]
    fn init_sets_manager_and_core() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(TransceiverContract, ());
        let client = TransceiverContractClient::new(&env, &contract_id);

        let manager = Address::generate(&env);
        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        let wormhole_core = Address::generate(&env);

        client.init(&manager, &manager_id, &wormhole_core);

        assert_eq!(client.get_manager(), manager);
        assert_eq!(client.get_wormhole_core(), wormhole_core);
    }

    #[test]
    fn set_manager_overrides_previous_value() {
        let (env, manager_id, _recipient_manager) = setup();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let manager = Address::generate(&env);
        let wormhole_core = Address::generate(&env);

        let mut init_args = Vec::new(&env);
        init_args.push_back(manager.into_val(&env));
        init_args.push_back(manager_id.into_val(&env));
        init_args.push_back(wormhole_core.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "init", init_args);
        transceiver_client.init(&manager, &manager_id, &wormhole_core);

        let new_manager = Address::generate(&env);
        let mut set_args = Vec::new(&env);
        set_args.push_back(new_manager.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "set_manager", set_args);

        transceiver_client.set_manager(&new_manager);
        assert_eq!(transceiver_client.get_manager(), new_manager);
    }

    #[test]
    fn set_peer_and_get_peer_roundtrip() {
        let (env, manager_id, _recipient_manager) = setup();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let manager = Address::generate(&env);
        let wormhole_core = Address::generate(&env);

        let mut init_args = Vec::new(&env);
        init_args.push_back(manager.into_val(&env));
        init_args.push_back(manager_id.into_val(&env));
        init_args.push_back(wormhole_core.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "init", init_args);
        transceiver_client.init(&manager, &manager_id, &wormhole_core);

        let chain_id: u32 = 2;
        let emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);
        let mut set_peer_args = Vec::new(&env);
        set_peer_args.push_back(chain_id.into_val(&env));
        set_peer_args.push_back(emitter.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "set_peer", set_peer_args);

        transceiver_client.set_peer(&chain_id, &emitter);

        let got = transceiver_client
            .get_peer(&chain_id)
            .expect("peer should be set");
        assert_eq!(got, emitter);
    }

    #[test]
    fn send_calls_core_and_returns_sequence() {
        let (env, manager_id, recipient_manager) = setup();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let core_id = env.register(DummyWormholeCore, ());
        let manager = Address::generate(&env);

        let mut init_args = Vec::new(&env);
        init_args.push_back(manager.into_val(&env));
        init_args.push_back(manager_id.into_val(&env));
        init_args.push_back(core_id.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "init", init_args);
        transceiver_client.init(&manager, &manager_id, &core_id);

        let dst_chain_id: u32 = 2;
        let manager_payload = Bytes::from_array(&env, &[1u8, 2, 3]);
        let mut send_args = Vec::new(&env);
        send_args.push_back(dst_chain_id.into_val(&env));
        send_args.push_back(recipient_manager.into_val(&env));
        send_args.push_back(manager_payload.into_val(&env));
        auth_invoke(&env, &manager, &transceiver_id, "send", send_args);

        let seq = transceiver_client.send(&dst_chain_id, &recipient_manager, &manager_payload);
        assert_eq!(seq, 42);
    }

    #[test]
    #[should_panic]
    fn send_panics_if_core_not_set() {
        let (env, _manager_id, recipient_manager) = setup();

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver_client = TransceiverContractClient::new(&env, &transceiver_id);

        let dst_chain_id: u32 = 2;
        let manager_payload = Bytes::from_array(&env, &[1u8, 2, 3]);

        let _ = transceiver_client.send(&dst_chain_id, &recipient_manager, &manager_payload);
    }

    #[test]
    #[should_panic]
    fn set_peer_requires_auth() {
        let env = Env::default();

        let contract_id = env.register(TransceiverContract, ());
        let client = TransceiverContractClient::new(&env, &contract_id);

        let manager = Address::generate(&env);
        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        let wormhole_core = Address::generate(&env);

        env.as_contract(&contract_id, || {
            set_manager_internal(&env, &manager);
            set_manager_id_internal(&env, &manager_id);
            set_wormhole_core_internal(&env, &wormhole_core);
        });

        let chain_id: u32 = 2;
        let emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);
        client.set_peer(&chain_id, &emitter);
    }

    #[test]
    fn encode_transceiver_message_layout() {
        let (env, _manager_id, _recipient_manager) = setup();

        let src = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let dst = BytesN::<32>::from_array(&env, &[2u8; 32]);
        let payload = Bytes::from_array(&env, &[9u8, 8u8, 7u8]);

        let out = encode_transceiver_message(&env, &src, &dst, &payload);

        let mut expected = Bytes::new(&env);
        expected.append(&Bytes::from_array(&env, &NTT_PREFIX.to_be_bytes()));
        expected.append(&Bytes::from_array(&env, &src.to_array()));
        expected.append(&Bytes::from_array(&env, &dst.to_array()));
        expected.append(&payload);

        assert_eq!(out, expected);
    }

    #[test]
    fn receive_vaa_happy_path_forwards_payload_to_manager() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        transceiver.init(&manager_addr, &manager_id, &core_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);

        let src_manager = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let inner_manager_payload = Bytes::from_array(&env, &[1u8, 2, 3, 4]);

        let tm_payload =
            encode_transceiver_message(&env, &src_manager, &manager_id, &inner_manager_payload);

        let seq: u64 = 1;
        core_client.set_parsed_vaa(&emitter_chain, &emitter_address, &seq, &tm_payload);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_vaa(&vaa_bytes);

        let mgr = DummyManagerClient::new(&env, &manager_contract_id);
        assert_eq!(mgr.last_src_chain().unwrap(), emitter_chain);
        assert_eq!(mgr.last_message().unwrap(), inner_manager_payload);
    }

    #[test]
    #[should_panic(expected = "no peer configured for emitter chain")]
    fn receive_vaa_panics_if_peer_missing() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        transceiver.init(&manager_addr, &manager_id, &core_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        let payload = Bytes::from_array(&env, &[1u8]);
        let seq: u64 = 1;
        core_client.set_parsed_vaa(&emitter_chain, &emitter_address, &seq, &payload);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_vaa(&vaa_bytes);
    }

    #[test]
    #[should_panic(expected = "unexpected emitter for chain")]
    fn receive_vaa_panics_if_emitter_mismatch() {
        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        transceiver.init(&manager_addr, &manager_id, &core_id);

        let emitter_chain: u32 = 2;

        let expected_emitter = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &expected_emitter);

        let wrong_emitter = BytesN::<32>::from_array(&env, &[8u8; 32]);
        let payload = Bytes::from_array(&env, &[1u8, 2, 3]);
        let seq: u64 = 1;
        core_client.set_parsed_vaa(&emitter_chain, &wrong_emitter, &seq, &payload);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);
        transceiver.receive_vaa(&vaa_bytes);
    }

    #[test]
    fn receive_vaa_panics_on_replay() {
        use core::panic::AssertUnwindSafe;

        let env = Env::default();
        env.mock_all_auths();

        let manager_contract_id = env.register(DummyManager, ());
        let manager_addr = manager_contract_id.clone();

        let core_id = env.register(DummyWormholeCore, ());
        let core_client = DummyWormholeCoreClient::new(&env, &core_id);

        let transceiver_id = env.register(TransceiverContract, ());
        let transceiver = TransceiverContractClient::new(&env, &transceiver_id);

        let manager_id = BytesN::<32>::from_array(&env, &[9u8; 32]);
        transceiver.init(&manager_addr, &manager_id, &core_id);

        let emitter_chain: u32 = 2;
        let emitter_address = BytesN::<32>::from_array(&env, &[7u8; 32]);
        transceiver.set_peer(&emitter_chain, &emitter_address);

        let src_mgr = BytesN::<32>::from_array(&env, &[1u8; 32]);
        let mgr_payload = Bytes::from_array(&env, &[1u8, 2, 3]);
        let tm_payload = encode_transceiver_message(&env, &src_mgr, &manager_id, &mgr_payload);

        let seq: u64 = 123;
        core_client.set_parsed_vaa(&emitter_chain, &emitter_address, &seq, &tm_payload);

        let vaa_bytes = Bytes::from_array(&env, &[0xaa]);

        transceiver.receive_vaa(&vaa_bytes);

        let res = catch_unwind(AssertUnwindSafe(|| {
            transceiver.receive_vaa(&vaa_bytes);
        }));
        assert!(res.is_err());

        use alloc::string::String;

        if let Err(e) = res {
            let s: String = if let Some(s) = e.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };

            assert!(s.contains("vaa already consumed"), "panic was: {s}");
        }

        // TODO: If you want deterministic replay failure strings for `#[should_panic(expected=...)]`,
        // prefer matching on the host error format used by your CI toolchain, or keep the current
        // `contains(...)` approach (less brittle across soroban host versions).
    }
}
