#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct ManagerContract;

#[contractimpl]
impl ManagerContract {
    pub fn init(_env: Env) {
        // TODO
    }
}
