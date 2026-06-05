#![cfg(test)]

mod common;

use common::manager::MockNttManagerClient;
use common::setup;
use common::wormhole::MockWormholeCoreClient;
use soroban_ntt_client::TransceiverError;
use soroban_sdk::Env;

const PEER_CHAIN: u32 = 2;

/// `quote_delivery_price` returns the core's message fee (as i128). With a
/// non-zero fee set, this catches a hardcoded zero or a stub that never queries
/// the core — which would silently break fee accounting for off-chain SDKs.
#[test]
fn quote_delivery_price_returns_core_fee() {
    let env = Env::default();
    let t = setup(&env);
    MockWormholeCoreClient::new(&env, &t.get_wormhole_core()).set_fee(&100);

    assert_eq!(t.quote_delivery_price(&PEER_CHAIN), 100);
}

/// When the core fee query fails, `quote_delivery_price` returns
/// `WormholeQueryFailed` rather than a default fee the caller then can't pay.
#[test]
fn quote_delivery_price_propagates_query_failure() {
    let env = Env::default();
    let t = setup(&env);
    MockWormholeCoreClient::new(&env, &t.get_wormhole_core()).fail_fee(&true);

    assert_eq!(
        t.try_quote_delivery_price(&PEER_CHAIN),
        Err(Ok(TransceiverError::WormholeQueryFailed))
    );
}

/// `get_manager_token` is a view that cross-calls the manager's `get_token`; it
/// returns the address the manager reports. Catches a regression that breaks the
/// cross-contract dispatch (bad client encoding).
#[test]
fn get_manager_token_forwards_to_manager() {
    let env = Env::default();
    let t = setup(&env);
    let manager = MockNttManagerClient::new(&env, &t.get_manager());

    assert_eq!(t.get_manager_token(), manager.get_token());
}

/// When the manager's `get_token` fails, `get_manager_token` surfaces
/// `ManagerQueryFailed` rather than a host-level panic — the only failing view,
/// and clients need a typed error.
#[test]
fn get_manager_token_propagates_manager_failure() {
    let env = Env::default();
    let t = setup(&env);
    MockNttManagerClient::new(&env, &t.get_manager()).fail_query(&true);

    assert_eq!(
        t.try_get_manager_token(),
        Err(Ok(TransceiverError::ManagerQueryFailed))
    );
}
