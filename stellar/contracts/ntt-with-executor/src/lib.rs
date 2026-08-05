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

mod encoding;
mod fee;

pub mod executor;

use executor::ExecutorClient;
use soroban_ntt_client::{NttManagerClient, TTL_EXTEND, TTL_THRESHOLD};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Bytes, BytesN, Env,
};

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
    /// The manager has no registered peer for the recipient chain.
    PeerNotFound = 2,
}

/// Cross-chain destination of the transfer.
#[derive(Clone)]
#[contracttype]
pub struct Destination {
    /// Wormhole chain id of the destination chain.
    pub chain: u32,
    /// 32-byte recipient address on the destination chain.
    pub recipient: BytesN<32>,
}

/// Referrer fee taken from the transfer amount before bridging.
#[derive(Clone)]
#[contracttype]
pub struct FeeArgs {
    /// Recipient of the fee, paid in the transfer token.
    pub referrer: Address,
    /// Fee rate in tenths of a basis point (denominator 100_000).
    pub dbps: u32,
}

/// Parameters forwarded verbatim to `Executor::request_execution`.
///
/// `payee`, `signed_quote`, and `relay_instructions` are produced off-chain by
/// the Executor API and validated by the executor, not by this contract.
#[derive(Clone)]
#[contracttype]
pub struct ExecutorArgs {
    /// Address the executor pays for the delivery, bound to the signed quote.
    pub payee: Address,
    /// XLM execution fee the executor pulls from the sender.
    pub amount: i128,
    /// Address that receives any relay refund on the destination chain.
    pub refund: Address,
    /// Opaque signed quote (EQ01) authorizing the fee.
    pub signed_quote: Bytes,
    /// Opaque relay instructions for the delivery provider.
    pub relay_instructions: Bytes,
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

    /// Transfers `amount` through `ntt_manager` and pays the executor to relay
    /// it, returning the manager's message sequence.
    ///
    /// All token movements — the optional referrer fee, the manager's
    /// lock/burn, and the executor's XLM fee — run under the single auth tree
    /// rooted at `sender`; the wrapper takes no custody. `should_queue` is
    /// forced to `false`: a rate-limited transfer emits no message, so paying a
    /// relayer for it would be premature and the whole call reverts instead.
    ///
    /// The executor's `payee`, `signed_quote`, and `relay_instructions` are
    /// opaque values the executor binds against the off-chain quote; the wrapper
    /// only forwards them.
    pub fn transfer(
        env: Env,
        sender: Address,
        ntt_manager: Address,
        amount: i128,
        destination: Destination,
        fee: FeeArgs,
        exec: ExecutorArgs,
    ) -> Result<u64, WrapperError> {
        sender.require_auth();

        let manager = NttManagerClient::new(&env, &ntt_manager);
        let peer = manager
            .get_peer(&destination.chain)
            .ok_or(WrapperError::PeerNotFound)?;

        let fee_amount = fee::referrer_fee(
            amount,
            fee.dbps,
            manager.token_decimals() as u8,
            peer.token_decimals as u8,
        )?;
        if fee_amount > 0 {
            token::Client::new(&env, &manager.get_token()).transfer(
                &sender,
                &fee.referrer,
                &fee_amount,
            );
        }

        let sequence = manager
            .transfer(
                &sender,
                &(amount - fee_amount),
                &destination.chain,
                &destination.recipient,
                &false,
            )
            .sequence;

        // The source chain id is fixed at manager construction to fit u16.
        let request =
            encoding::ntt_request(&env, manager.get_chain_id() as u16, &ntt_manager, sequence);
        ExecutorClient::new(&env, &executor_address(&env)).request_execution(
            &destination.chain,
            &peer.address,
            &exec.refund,
            &sender,
            &exec.payee,
            &exec.amount,
            &exec.signed_quote,
            &request,
            &exec.relay_instructions,
        );

        Ok(sequence)
    }
}

fn executor_address(env: &Env) -> Address {
    let storage = env.storage().instance();
    storage.extend_ttl(TTL_THRESHOLD, TTL_EXTEND);
    storage.get(&DataKey::Executor).unwrap()
}
