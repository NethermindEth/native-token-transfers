#![no_std]

//! Wrapper contract that initiates an NTT transfer and registers a matching
//! execution request with the Wormhole Executor in a single Stellar transaction.
//!
//! A Soroban transaction permits only one top-level `InvokeHostFunction`, so an
//! NTT transfer and its `Executor::request_execution` call cannot be batched
//! client-side. This contract orchestrates both under the sender's auth tree,
//! mirroring the EVM `NttManagerWithExecutor` and Solana `example_ntt_with_executor`
//! shims: it forwards the transfer to the manager and pays the relay provider so
//! the transfer is auto-delivered on the destination chain.
//!
//! The executor is bound at construction (one per chain); the NTT manager is a
//! per-call argument, so a single wrapper serves every manager on the chain.

pub mod fee;

use soroban_ntt_client::{TTL_EXTEND, TTL_THRESHOLD};
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

/// Errors raised by the wrapper's own validation. Manager and executor failures
/// trap through their typed clients and revert the whole transaction.
///
/// Discriminants are part of the on-chain ABI: append only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum WrapperError {
    /// Referrer fee `dbps` exceeds the `u16` on-wire fee field.
    InvalidReferrerFee = 1,
}

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Executor,
}

#[contract]
pub struct NttWithExecutor;

#[contractimpl]
impl NttWithExecutor {
    pub fn __constructor(env: Env, executor: Address) {
        env.storage().instance().set(&DataKey::Executor, &executor);
    }
}

pub fn executor(env: &Env) -> Address {
    let storage = env.storage().instance();
    storage.extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
    storage.get(&DataKey::Executor).unwrap()
}
