use soroban_ntt_client::Mode;
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellar_ntt_manager::{ManagerContract, ManagerContractClient};

/// Deploys a manager over an arbitrary token. Returns the owner and a typed client.
pub fn setup_manager_with_token<'a>(
    env: &Env,
    mode: Mode,
    chain_id: u32,
    outbound_limit: u64,
    rate_limit_duration: u64,
    token: &Address,
) -> (Address, ManagerContractClient<'a>) {
    let owner = Address::generate(env);
    let manager = env.register(
        ManagerContract,
        (
            &owner,
            token,
            &mode,
            &chain_id,
            &outbound_limit,
            &rate_limit_duration,
        ),
    );
    (owner, ManagerContractClient::new(env, &manager))
}

/// Deploys a manager backed by a 7-decimal Stellar Asset Contract token.
/// Returns the owner, token address, and a typed client.
pub fn setup_manager<'a>(
    env: &Env,
    mode: Mode,
    chain_id: u32,
    outbound_limit: u64,
    rate_limit_duration: u64,
) -> (Address, Address, ManagerContractClient<'a>) {
    let token = env
        .register_stellar_asset_contract_v2(Address::generate(env))
        .address();
    let (owner, client) =
        setup_manager_with_token(env, mode, chain_id, outbound_limit, rate_limit_duration, &token);
    (owner, token, client)
}
