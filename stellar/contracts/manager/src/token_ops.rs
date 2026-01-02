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
//! Burning mode requires a custom token contract that implements:
//!
//! - `burn(from: Address, amount: i128)` - Burns tokens from the sender
//! - `mint(to: Address, amount: i128)` - Mints tokens to the recipient
//!
//! The NTT Manager contract address must be authorized as a minter/burner
//! on the token contract. Standard Stellar Asset Contracts do NOT support
//! burn/mint operations with external authorization.
//!
//! ## Recommendation
//!
//! - Use **Locking mode** with existing SEP-41 tokens (simplest setup)
//! - Use **Burning mode** only with a custom NTT-compatible token contract

#![allow(dead_code)]

use soroban_sdk::{token, vec, Address, Env, IntoVal, Symbol};

use crate::errors::NttManagerError;
use crate::state::{DataKey, Mode};

/// Retrieves the configured token address from storage.
///
/// Returns `NotInitialized` if the contract has not been initialized.
pub fn get_token(env: &Env) -> Result<Address, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::Token)
        .ok_or(NttManagerError::NotInitialized)
}

/// Retrieves the operating mode (Locking or Burning) from storage.
///
/// Returns `NotInitialized` if the contract has not been initialized.
pub fn get_mode(env: &Env) -> Result<Mode, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::Mode)
        .ok_or(NttManagerError::NotInitialized)
}

/// Retrieves the cached token decimals from storage.
///
/// Decimals are queried once during initialization and cached. This avoids
/// repeated cross-contract calls when normalizing transfer amounts.
///
/// Returns `NotInitialized` if the contract has not been initialized.
pub fn get_token_decimals(env: &Env) -> Result<u32, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::TokenDecimals)
        .ok_or(NttManagerError::NotInitialized)
}

/// Queries the token balance for an account.
///
/// Makes a cross-contract call to the configured token contract.
pub fn get_token_balance(env: &Env, account: &Address) -> Result<i128, NttManagerError> {
    let token_addr = get_token(env)?;
    let client = token::Client::new(env, &token_addr);
    Ok(client.balance(account))
}

/// Transfers tokens from the sender to this contract (locking mode).
///
/// Used for outbound transfers on the canonical chain. The sender must have
/// previously authorized the transfer via `require_auth`.
pub fn lock_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    let contract = env.current_contract_address();
    let client = token::Client::new(env, &token_addr);
    client.transfer(from, &contract, &amount);
    Ok(())
}

/// Transfers tokens from this contract to the recipient (locking mode).
///
/// Used for inbound transfers on the canonical chain to release previously
/// locked tokens to the destination address.
pub fn unlock_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    let contract = env.current_contract_address();
    let client = token::Client::new(env, &token_addr);
    client.transfer(&contract, to, &amount);
    Ok(())
}

/// Burns tokens from the sender (burning mode).
///
/// Used for outbound transfers on non-canonical chains. Requires a custom
/// token contract that implements `burn(from, amount)` and has authorized
/// this NTT Manager as a burner.
pub fn burn_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    env.invoke_contract::<()>(
        &token_addr,
        &Symbol::new(env, "burn"),
        vec![env, from.into_val(env), amount.into_val(env)],
    );
    Ok(())
}

/// Mints tokens to the recipient (burning mode).
///
/// Used for inbound transfers on non-canonical chains. Requires a custom
/// token contract that implements `mint(to, amount)` and has authorized
/// this NTT Manager as a minter.
pub fn mint_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    env.invoke_contract::<()>(
        &token_addr,
        &Symbol::new(env, "mint"),
        vec![env, to.into_val(env), amount.into_val(env)],
    );
    Ok(())
}

/// Takes custody of tokens for an outbound transfer.
///
/// Dispatches to `lock_tokens` or `burn_tokens` based on the configured mode.
/// This is the primary entry point for outbound token operations.
pub fn custody_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let mode = get_mode(env)?;
    match mode {
        Mode::Locking => lock_tokens(env, from, amount),
        Mode::Burning => burn_tokens(env, from, amount),
    }
}

/// Releases tokens to the recipient for an inbound transfer.
///
/// Dispatches to `unlock_tokens` or `mint_tokens` based on the configured mode.
/// This is the primary entry point for inbound token operations.
pub fn release_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let mode = get_mode(env)?;
    match mode {
        Mode::Locking => unlock_tokens(env, to, amount),
        Mode::Burning => mint_tokens(env, to, amount),
    }
}
