use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env};

/// Configurable-decimals token stand-in tracking per-address balances. Implements
/// only the SEP-41 surface the manager touches: decimals, balance, transfer,
/// burn, mint.
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

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&id).unwrap_or(0)
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        let updated = Self::balance(env.clone(), to.clone()) + amount;
        env.storage().persistent().set(&to, &updated);
    }

    pub fn burn(env: Env, from: Address, amount: i128) {
        let updated = Self::balance(env.clone(), from.clone()) - amount;
        env.storage().persistent().set(&from, &updated);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let from_balance = Self::balance(env.clone(), from.clone()) - amount;
        let to_balance = Self::balance(env.clone(), to.clone()) + amount;
        env.storage().persistent().set(&from, &from_balance);
        env.storage().persistent().set(&to, &to_balance);
    }
}
