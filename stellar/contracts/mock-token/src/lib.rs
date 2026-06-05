//! Parameterised-decimals fungible token used as a fixture by the NTT
//! integration tests.
//!
//! Implements the subset of the Soroban token interface the NTT manager
//! exercises (`decimals`, `balance`, `transfer`, `burn`) plus a
//! permission-less `mint` so tests can fund senders without admin-key
//! ceremony. Decimals are supplied at construction and stored in instance
//! storage; balances live in persistent storage keyed by `Address`.
//!
//! Production tokens an NTT deployment plans to bridge should use the
//! Stellar Asset Contract or a custom SEP-41 implementation — this contract
//! is intentionally minimal and not hardened for any other use.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
enum DataKey {
    Decimals,
    Balance(Address),
}

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    /// Stores the token's decimal precision in instance storage. Called once
    /// at deploy time.
    pub fn __constructor(env: Env, decimals: u32) {
        env.storage().instance().set(&DataKey::Decimals, &decimals);
    }

    /// Returns the decimal precision configured at deploy time.
    pub fn decimals(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Decimals).unwrap()
    }

    /// Returns `id`'s balance, or `0` if no entry exists for that address.
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    /// Credits `amount` to `to` without any auth check. Test-only — a
    /// production token must gate mint on admin auth.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let current = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(current + amount));
    }

    /// Debits `amount` from `from`. Requires `from`'s auth. Panics if the
    /// balance is below `amount`.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        let current = Self::balance(env.clone(), from.clone());
        if current < amount {
            panic!("insufficient balance");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(current - amount));
    }

    /// Moves `amount` from `from` to `to`. Requires `from`'s auth. Panics if
    /// `from`'s balance is below `amount`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("insufficient balance");
        }
        let to_balance = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(to_balance + amount));
    }
}
