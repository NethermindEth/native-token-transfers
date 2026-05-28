use soroban_sdk::{contract, contractimpl, symbol_short, Env};

/// Minimal token stand-in with configurable decimals. Balance/mint/burn/transfer
/// are added in the commits that exercise token custody flows.
#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn __constructor(env: Env, decimals: u32) {
        env.storage().instance().set(&symbol_short!("dec"), &decimals);
    }

    pub fn decimals(env: Env) -> u32 {
        env.storage().instance().get(&symbol_short!("dec")).unwrap()
    }
}
