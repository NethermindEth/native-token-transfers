use soroban_ntt_client::{flatten_call, native_sac};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token::TokenClient, Address, Bytes, BytesN,
    Env, Symbol, Vec,
};
use wormhole_soroban_client::{WormholeError, VAA};

const CFG: Symbol = symbol_short!("cfg");
const POSTED: Symbol = symbol_short!("posted");

/// The parsed VAA `parse_and_verify_vaa` returns, plus failure knobs and the
/// message fee. Tests set the parsed fields with `set_vaa` and
/// `set_consistency_level`, and flip flags with the `fail_*` setters.
#[contracttype]
#[derive(Clone)]
struct Config {
    emitter_chain: u32,
    emitter_address: BytesN<32>,
    sequence: u64,
    consistency_level: u32,
    payload: Bytes,
    fee: u64,
    fail_verify: bool,
    fail_post: bool,
    fail_fee: bool,
}

/// The last message the transceiver posted, recorded so outbound and broadcast
/// tests can assert the exact bytes handed to the core.
#[contracttype]
pub struct LastPosted {
    pub emitter: Address,
    pub nonce: u32,
    pub payload: Bytes,
    pub consistency_level: u32,
}

/// Wormhole-core stand-in exposing the three methods the transceiver calls:
/// `parse_and_verify_vaa` returns the configured VAA (or fails verification),
/// `post_message` collects the fee and records what it received, and
/// `get_message_fee` returns the configured fee. Real signature verification
/// lives in the IT suite.
#[contract]
pub struct MockWormholeCore;

#[contractimpl]
impl MockWormholeCore {
    pub fn __constructor(env: Env, fee: u64) {
        env.storage().instance().set(
            &CFG,
            &Config {
                emitter_chain: 0,
                emitter_address: BytesN::from_array(&env, &[0; 32]),
                sequence: 0,
                consistency_level: 1,
                payload: Bytes::new(&env),
                fee,
                fail_verify: false,
                fail_post: false,
                fail_fee: false,
            },
        );
    }

    pub fn set_vaa(env: Env, emitter_chain: u32, emitter_address: BytesN<32>, sequence: u64, payload: Bytes) {
        let mut c = cfg(&env);
        (c.emitter_chain, c.emitter_address, c.sequence, c.payload) =
            (emitter_chain, emitter_address, sequence, payload);
        env.storage().instance().set(&CFG, &c);
    }

    pub fn set_consistency_level(env: Env, consistency_level: u32) {
        let mut c = cfg(&env);
        c.consistency_level = consistency_level;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn set_fee(env: Env, fee: u64) {
        let mut c = cfg(&env);
        c.fee = fee;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn fail_verify(env: Env, fail: bool) {
        let mut c = cfg(&env);
        c.fail_verify = fail;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn fail_post(env: Env, fail: bool) {
        let mut c = cfg(&env);
        c.fail_post = fail;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn fail_fee(env: Env, fail: bool) {
        let mut c = cfg(&env);
        c.fail_fee = fail;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn parse_and_verify_vaa(env: Env, _vaa_bytes: Bytes) -> Result<VAA, WormholeError> {
        let c = cfg(&env);
        if c.fail_verify {
            return Err(WormholeError::InvalidSignature);
        }
        Ok(VAA {
            version: 1,
            guardian_set_index: 0,
            signatures: Vec::new(&env),
            timestamp: 0,
            nonce: 0,
            emitter_chain: c.emitter_chain,
            emitter_address: c.emitter_address,
            sequence: c.sequence,
            consistency_level: c.consistency_level,
            payload: c.payload,
        })
    }

    pub fn post_message(
        env: Env,
        emitter: Address,
        nonce: u32,
        payload: Bytes,
        consistency_level: u32,
    ) -> Result<u64, WormholeError> {
        let c = cfg(&env);
        if c.fail_post {
            return Err(WormholeError::InsufficientFeePaid);
        }
        if c.fee > 0 {
            // Collects the fee as the core does, from the emitter's allowance.
            let this = env.current_contract_address();
            let xlm = TokenClient::new(&env, &native_sac(&env));
            flatten_call(
                xlm.try_transfer_from(&this, &emitter, &this, &i128::from(c.fee)),
                WormholeError::InsufficientFeePaid,
            )?;
        }
        env.storage().instance().set(
            &POSTED,
            &LastPosted {
                emitter,
                nonce,
                payload,
                consistency_level,
            },
        );
        Ok(c.sequence)
    }

    pub fn get_message_fee(env: Env) -> u64 {
        let c = cfg(&env);
        if c.fail_fee {
            panic!("wormhole core message-fee query failed");
        }
        c.fee
    }

    pub fn last_posted(env: Env) -> Option<LastPosted> {
        env.storage().instance().get(&POSTED)
    }
}

fn cfg(env: &Env) -> Config {
    env.storage().instance().get(&CFG).unwrap()
}
