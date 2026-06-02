#![cfg(test)]

mod common;
#[path = "common/vaa.rs"]
mod vaa;

use common::setup_transceiver;
use soroban_ntt_client::TransceiverError;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};
use stellar_ntt_transceiver::{TransceiverContract, TransceiverContractClient};
use vaa::signed_inbound_vaa;
use wormhole_soroban_client::{
    WormholeClient, ACTION_SET_MESSAGE_FEE, CHAIN_ID_STELLAR, GOVERNANCE_CHAIN_ID,
    GOVERNANCE_EMITTER, MODULE_CORE,
};

const DEST_CHAIN: u32 = 6;

/// Sets the wormhole core's message fee via a guardian-signed governance VAA, so
/// the quote test runs against a non-zero fee.
fn set_core_fee(env: &Env, core: &WormholeClient, fee: u64) {
    let mut payload = Bytes::from_array(env, &MODULE_CORE);
    payload.push_back(ACTION_SET_MESSAGE_FEE);
    payload.append(&Bytes::from_slice(env, &CHAIN_ID_STELLAR.to_be_bytes()));
    payload.append(&Bytes::from_array(env, &[0u8; 24]));
    payload.append(&Bytes::from_slice(env, &fee.to_be_bytes()));
    let vaa = signed_inbound_vaa(
        env,
        GOVERNANCE_CHAIN_ID as u16,
        &BytesN::from_array(env, &GOVERNANCE_EMITTER),
        1,
        &payload,
    );
    core.submit_set_message_fee(&vaa);
}

/// `quote_delivery_price` returns the core's message fee (as i128). With a
/// non-zero fee set, this catches a hardcoded zero or stub that never queries the
/// core — which would silently break fee accounting for off-chain SDKs.
#[test]
fn quote_delivery_price_returns_core_fee() {
    let env = Env::default();
    let (_, _, transceiver, _, core) = setup_transceiver(&env);
    set_core_fee(&env, &core, 100);

    assert_eq!(
        transceiver.quote_delivery_price(&DEST_CHAIN),
        core.get_message_fee() as i128
    );
}

/// When the core fee query fails, `quote_delivery_price` returns
/// `WormholeQueryFailed` rather than a default fee the caller then can't pay. A
/// transceiver pointed at a core that cannot service the call exercises it.
#[test]
fn quote_delivery_price_propagates_query_failure() {
    let env = Env::default();
    let (_, _, _, manager, _) = setup_transceiver(&env);
    let owner = Address::generate(&env);
    let broken_core = Address::generate(&env);
    let transceiver = TransceiverContractClient::new(
        &env,
        &env.register(TransceiverContract, (&owner, &manager.address, &broken_core)),
    );

    assert_eq!(
        transceiver.try_quote_delivery_price(&DEST_CHAIN),
        Err(Ok(TransceiverError::WormholeQueryFailed))
    );
}

/// `get_manager_token` is a view that cross-calls the manager's `get_token`; it
/// returns the address the manager reports. Catches a regression that breaks the
/// cross-contract dispatch (bad client encoding).
#[test]
fn get_manager_token_forwards_to_manager() {
    let env = Env::default();
    let (_, _, transceiver, manager, _) = setup_transceiver(&env);

    assert_eq!(transceiver.get_manager_token(), manager.get_token());
}

/// When the manager's `get_token` fails, `get_manager_token` surfaces
/// `ManagerQueryFailed` rather than a host-level panic — the only failing view,
/// and clients need a typed error.
#[test]
fn get_manager_token_propagates_manager_failure() {
    let env = Env::default();
    let (_, _, transceiver, manager, _) = setup_transceiver(&env);

    let mut cfg = manager.config();
    cfg.fail_query = true;
    manager.configure(&cfg);

    assert_eq!(
        transceiver.try_get_manager_token(),
        Err(Ok(TransceiverError::ManagerQueryFailed))
    );
}
