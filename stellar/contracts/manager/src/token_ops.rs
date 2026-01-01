use soroban_sdk::{token, vec, Address, Env, IntoVal, Symbol};

use crate::errors::NttManagerError;
use crate::state::{DataKey, Mode};

pub fn get_token(env: &Env) -> Result<Address, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::Token)
        .ok_or(NttManagerError::NotInitialized)
}

pub fn get_mode(env: &Env) -> Result<Mode, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::Mode)
        .ok_or(NttManagerError::NotInitialized)
}

pub fn get_token_decimals(env: &Env) -> Result<u32, NttManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::TokenDecimals)
        .ok_or(NttManagerError::NotInitialized)
}

pub fn get_token_balance(env: &Env, account: &Address) -> Result<i128, NttManagerError> {
    let token_addr = get_token(env)?;
    let client = token::Client::new(env, &token_addr);
    Ok(client.balance(account))
}

pub fn lock_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    let contract = env.current_contract_address();
    let client = token::Client::new(env, &token_addr);
    client.transfer(from, &contract, &amount);
    Ok(())
}

pub fn unlock_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    let contract = env.current_contract_address();
    let client = token::Client::new(env, &token_addr);
    client.transfer(&contract, to, &amount);
    Ok(())
}

pub fn burn_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    env.invoke_contract::<()>(
        &token_addr,
        &Symbol::new(env, "burn"),
        vec![env, from.into_val(env), amount.into_val(env)],
    );
    Ok(())
}

pub fn mint_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let token_addr = get_token(env)?;
    env.invoke_contract::<()>(
        &token_addr,
        &Symbol::new(env, "mint"),
        vec![env, to.into_val(env), amount.into_val(env)],
    );
    Ok(())
}

pub fn custody_tokens(env: &Env, from: &Address, amount: i128) -> Result<(), NttManagerError> {
    let mode = get_mode(env)?;
    match mode {
        Mode::Locking => lock_tokens(env, from, amount),
        Mode::Burning => burn_tokens(env, from, amount),
    }
}

pub fn release_tokens(env: &Env, to: &Address, amount: i128) -> Result<(), NttManagerError> {
    let mode = get_mode(env)?;
    match mode {
        Mode::Locking => unlock_tokens(env, to, amount),
        Mode::Burning => mint_tokens(env, to, amount),
    }
}
