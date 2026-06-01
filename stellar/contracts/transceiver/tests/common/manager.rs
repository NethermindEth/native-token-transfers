use soroban_ntt_client::{AttestationResult, Mode, NttManagerError};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env, Symbol,
};

const CFG: Symbol = symbol_short!("cfg");
const LAST: Symbol = symbol_short!("last");

/// Behaviour knobs for [`MockNttManager`], stored under one key; tests read with
/// `config`, mutate a field, and write it back with `configure`.
#[contracttype]
pub struct MockManagerConfig {
    pub token: Address,
    pub mode: Mode,
    pub decimals: u32,
    pub fail_attestation: bool,
    pub fail_query: bool,
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

/// Manager stand-in exposing only the methods the transceiver calls through
/// `NttManagerClient`: the inbound `attestation_received` and the `get_token` /
/// `get_mode` / `token_decimals` queries `broadcast_id` reads.
#[contract]
pub struct MockNttManager;

#[contractimpl]
impl MockNttManager {
    pub fn __constructor(env: Env, config: MockManagerConfig) {
        env.storage().instance().set(&CFG, &config);
    }

    pub fn configure(env: Env, config: MockManagerConfig) {
        env.storage().instance().set(&CFG, &config);
    }

    pub fn config(env: Env) -> MockManagerConfig {
        env.storage().instance().get(&CFG).unwrap()
    }

    pub fn attestation_received(
        env: Env,
        transceiver: Address,
        source_chain: u32,
        source_ntt_manager: BytesN<32>,
        payload: Bytes,
    ) -> Result<AttestationResult, NttManagerError> {
        let cfg: MockManagerConfig = env.storage().instance().get(&CFG).unwrap();
        if cfg.fail_attestation {
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
        Ok(query_config(&env)?.token)
    }

    pub fn get_mode(env: Env) -> Result<Mode, NttManagerError> {
        Ok(query_config(&env)?.mode)
    }

    pub fn token_decimals(env: Env) -> Result<u32, NttManagerError> {
        Ok(query_config(&env)?.decimals)
    }

    pub fn last_attestation(env: Env) -> Option<LastAttestation> {
        env.storage().instance().get(&LAST)
    }
}

/// Loads the config for a read query, surfacing `fail_query` as the same
/// host-level failure a real manager query would, so the transceiver maps it to
/// `ManagerQueryFailed`.
fn query_config(env: &Env) -> Result<MockManagerConfig, NttManagerError> {
    let cfg: MockManagerConfig = env.storage().instance().get(&CFG).unwrap();
    if cfg.fail_query {
        return Err(NttManagerError::NotInitialized);
    }
    Ok(cfg)
}
