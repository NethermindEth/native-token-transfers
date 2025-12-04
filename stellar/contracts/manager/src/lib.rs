#![no_std]

use soroban_sdk::{contract, contractimpl, Bytes, BytesN, Env};

#[contract]
pub struct ManagerContract;

#[contractimpl]
impl ManagerContract {
    pub fn init(_env: Env) {
        // TODO
    }

    // Called by Transceiver after wormhole verification + peer transceiver check
    pub fn receive_wormhole_message(
        _env: Env,
        _emitter_chain: u32,
        _emitter_address: BytesN<32>,
        _sequence: u64,
        _payload: Bytes,
    ) {
        // TODO
    }
}
