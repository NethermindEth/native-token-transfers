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
        read(&env, &id)
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        write(&env, &to, read(&env, &to) + amount);
    }

    pub fn burn(env: Env, from: Address, amount: i128) {
        write(&env, &from, read(&env, &from) - amount);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        write(&env, &from, read(&env, &from) - amount);
        write(&env, &to, read(&env, &to) + amount);
    }
}

fn read(env: &Env, id: &Address) -> i128 {
    env.storage().persistent().get(id).unwrap_or(0)
}

fn write(env: &Env, id: &Address, value: i128) {
    env.storage().persistent().set(id, &value);
}
