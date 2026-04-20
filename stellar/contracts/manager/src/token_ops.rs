//! Token operations for the NTT Manager.
//!
//! # Token Contract Requirements
//!
//! ## Locking Mode
//!
//! Standard SEP-41 tokens (Stellar Asset Contracts) are fully supported.
//! The NTT Manager contract requires no special permissions—it uses the
//! standard `transfer` function to move tokens between accounts and itself.
//!
//! ## Burning Mode
//!
//! Burning mode uses two token operations:
//!
//! - `burn(from, amount)` — Part of the standard `TokenInterface` (SEP-41).
//!   The sender authorizes the burn of their own tokens. Works with any
//!   compliant token, including Stellar Asset Contracts.
//! - `mint(to, amount)` — Part of the `StellarAssetInterface` (admin-only).
//!   The NTT Manager must be set as the token admin (or the token contract
//!   must have custom authorization allowing the NTT Manager to mint).
//!
//! ## Recommendation
//!
//! - Use **Locking mode** with existing SEP-41 tokens (simplest setup)
//! - Use **Burning mode** only with a custom NTT-compatible token contract

use soroban_ntt_client::NttManagerError;
use soroban_sdk::{token, Address, Env};

use crate::{state::Mode, storage::InstanceStorage};

/// Queries the token contract for its decimal precision.
///
/// Makes a cross-contract call to the token's `decimals()` function.
/// Used during initialization to cache the token decimals in storage.
pub fn query_token_decimals(env: &Env, token: &Address) -> u32 {
    let client = token::Client::new(env, token);
    client.decimals()
}

/// Retrieves the cached token decimals from storage.
///
/// Decimals are queried once during initialization and cached. This avoids
/// repeated cross-contract calls when normalizing transfer amounts.
///
/// Returns `NotInitialized` if the contract has not been initialized.
pub fn get_token_decimals(env: &Env) -> Result<u32, NttManagerError> {
    InstanceStorage::new(env).token_decimals()
}

/// Queries the token balance for an account.
///
/// Makes a cross-contract call to the configured token contract.
pub fn get_token_balance(env: &Env, account: &Address) -> Result<i128, NttManagerError> {
    let token_addr = InstanceStorage::new(env).token()?;
    let client = token::Client::new(env, &token_addr);
    Ok(client.balance(account))
}

/// Transfers tokens from the sender to this contract (locking mode).
///
/// Used for outbound transfers on the canonical chain. The sender must have
/// previously authorized the transfer via `require_auth`.
fn lock_tokens(env: &Env, token_addr: &Address, from: &Address, amount: i128) {
    let contract = env.current_contract_address();
    let client = token::Client::new(env, token_addr);
    client.transfer(from, &contract, &amount);
}

/// Transfers tokens from this contract to the recipient (locking mode).
///
/// Used for inbound transfers on the canonical chain to release previously
/// locked tokens to the destination address.
fn unlock_tokens(env: &Env, token_addr: &Address, to: &Address, amount: i128) {
    let contract = env.current_contract_address();
    let client = token::Client::new(env, token_addr);
    client.transfer(&contract, to, &amount);
}

/// Burns tokens from the sender (burning mode).
///
/// Uses the standard `TokenInterface::burn` — the sender authorizes the
/// burn of their own tokens. Works with any SEP-41 compliant token.
fn burn_tokens(env: &Env, token_addr: &Address, from: &Address, amount: i128) {
    let client = token::Client::new(env, token_addr);
    client.burn(from, &amount);
}

/// Mints tokens to the recipient (burning mode).
///
/// Uses `StellarAssetInterface::mint` — an admin-only operation. The NTT
/// Manager must be the token admin or otherwise authorized to mint.
fn mint_tokens(env: &Env, token_addr: &Address, to: &Address, amount: i128) {
    let client = token::StellarAssetClient::new(env, token_addr);
    client.mint(to, &amount);
}

/// Takes custody of tokens for an outbound transfer.
///
/// Dispatches to `lock_tokens` or `burn_tokens` based on the configured mode.
/// This is the primary entry point for outbound token operations.
pub fn custody_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let storage = InstanceStorage::new(env);
    let token_addr = storage.token()?;
    let mode = storage.mode()?;
    match mode {
        Mode::Locking => lock_tokens(env, &token_addr, from, amount),
        Mode::Burning => burn_tokens(env, &token_addr, from, amount),
    }
    Ok(())
}

/// Releases tokens to the recipient for an inbound transfer.
///
/// Dispatches to `unlock_tokens` or `mint_tokens` based on the configured mode.
/// This is the primary entry point for inbound token operations.
pub fn release_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let storage = InstanceStorage::new(env);
    let token_addr = storage.token()?;
    let mode = storage.mode()?;
    match mode {
        Mode::Locking => unlock_tokens(env, &token_addr, to, amount),
        Mode::Burning => mint_tokens(env, &token_addr, to, amount),
    }
    Ok(())
}
