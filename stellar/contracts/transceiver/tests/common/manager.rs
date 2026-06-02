use soroban_ntt_client::{AttestationResult, Mode, NttManagerError};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env, Symbol,
};

const CFG: Symbol = symbol_short!("cfg");
const LAST: Symbol = symbol_short!("last");

#[contracttype]
#[derive(Clone)]
struct Config {
    token: Address,
    mode: Mode,
    decimals: u32,
    fail_attestation: bool,
    fail_query: bool,
}

/// What the transceiver forwarded to `attestation_received`, recorded so inbound
/// tests assert the VAA was decoded into the right manager call.
#[contracttype]
pub struct LastAttestation {
    pub transceiver: Address,
    pub source_chain: u32,
    pub source_manager: BytesN<32>,
    pub payload: Bytes,
}

/// Manager stand-in exposing only the methods the transceiver calls: the inbound
/// `attestation_received` and the `get_token` / `get_mode` / `token_decimals`
/// queries `broadcast_id` reads. `fail_attestation` / `fail_query` inject the
/// rejection and query-failure paths.
#[contract]
pub struct MockNttManager;

#[contractimpl]
impl MockNttManager {
    pub fn __constructor(env: Env, token: Address, mode: Mode, decimals: u32) {
        env.storage().instance().set(
            &CFG,
            &Config { token, mode, decimals, fail_attestation: false, fail_query: false },
        );
    }

    pub fn fail_attestation(env: Env, fail: bool) {
        let mut c = cfg(&env);
        c.fail_attestation = fail;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn fail_query(env: Env, fail: bool) {
        let mut c = cfg(&env);
        c.fail_query = fail;
        env.storage().instance().set(&CFG, &c);
    }

    pub fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        if cfg(&env).fail_attestation {
            return Err(NttManagerError::TransceiverAlreadyAttested);
        }
        env.storage().instance().set(
            &LAST,
            &LastAttestation {
                transceiver,
                source_chain,
                source_manager: source_ntt_manager,
                payload,
            },
        );
        Ok(AttestationResult::executed())
    }

    pub fn get_token(env: Env) -> Result<Address, NttManagerError> {
        Ok(queryable(&env)?.token)
    }

    pub fn get_mode(env: Env) -> Result<Mode, NttManagerError> {
        Ok(queryable(&env)?.mode)
    }

    pub fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        Ok(queryable(&env)?.decimals)
    }

    pub fn last_attestation(env: Env) -> Option<LastAttestation> {
        env.storage().instance().get(&LAST)
    }
}

fn cfg(env: &Env) -> Config {
    env.storage().instance().get(&CFG).unwrap()
}

/// Loads the config for a read query, surfacing `fail_query` as the same
/// host-level failure a real manager query would, so the transceiver maps it to
/// `ManagerQueryFailed`.
fn queryable(env: &Env) -> Result<Config, NttManagerError> {
    let c = cfg(env);
    if c.fail_query {
        return Err(NttManagerError::NotInitialized);
    }
    Ok(c)
}
