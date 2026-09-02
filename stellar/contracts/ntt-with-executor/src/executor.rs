//! Client binding for the Wormhole Executor's delivery-request entrypoint.
//!
//! The upstream `executor-soroban-client` targets a newer soroban-sdk than this
//! workspace pins, so the binding is regenerated here against the workspace SDK.
//! The generated cross-contract call is identical on the wire.

use soroban_sdk::{contractclient, contracterror, Address, Bytes, BytesN, Env};

/// Errors returned by `Executor::request_execution`.
#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ExecutorError {
    /// Quote `expiryTime` is at or before the current ledger timestamp.
    QuoteExpired = 11,
    /// Quote `srcChain` does not match the executor's configured chain.
    QuoteSrcChainMismatch = 12,
    /// Quote `dstChain` does not match the `dst_chain` argument.
    QuoteDstChainMismatch = 13,
    /// The `amount` argument is negative.
    InvalidAmount = 14,
    /// The signed quote is shorter than its 68-byte header.
    InvalidQuote = 15,
    /// The `payee` does not match the payee signed in the quote.
    QuotePayeeMismatch = 16,
}

#[contractclient(name = "ExecutorClient")]
pub trait ExecutorInterface {
    fn request_execution(
        env: Env,
        dst_chain: u32,
        dst_addr: BytesN<32>,
        refund: Address,
        payer: Address,
        payee: Address,
        amount: i128,
        signed_quote_bytes: Bytes,
        request: Bytes,
        relay_instructions: Bytes,
    ) -> Result<(), ExecutorError>;
}
