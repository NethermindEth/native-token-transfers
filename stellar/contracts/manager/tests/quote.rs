#![cfg(test)]

mod common;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::setup_manager;
use soroban_ntt_client::{Mode, NttManagerError};
use soroban_sdk::{BytesN, Env};
use transceiver::add_transceiver;

/// `quote_transfer` rejects an unregistered recipient chain (`PeerNotFound`)
/// and a chain id beyond the `u16` Wormhole range (`ChainIdTooLarge`).
#[test]
fn quote_transfer_rejects_bad_inputs() {
    let env = Env::default();
    let (_, _, client) = setup_manager(&env, Mode::Locking, 1, 1000, 3600);

    assert_eq!(
        client.try_quote_transfer(&100, &2),
        Err(Ok(NttManagerError::PeerNotFound))
    );
    assert_eq!(
        client.try_quote_transfer(&100, &70_000),
        Err(Ok(NttManagerError::ChainIdTooLarge))
    );
}

/// `quote_transfer` validates its amount like `transfer` does, so a quote can
/// never accept an amount the transfer would reject: zero and a negative amount
/// (which would wrap through the `i128 as u128` cast into a bogus large value)
/// both fail with `ZeroAmount` before any trimming.
#[test]
fn quote_transfer_rejects_non_positive_amount() {
    let env = Env::default();
    let (_, _, client) = setup_manager(&env, Mode::Locking, 1, 1000, 3600);

    assert_eq!(
        client.try_quote_transfer(&0, &2),
        Err(Ok(NttManagerError::ZeroAmount))
    );
    assert_eq!(
        client.try_quote_transfer(&-1, &2),
        Err(Ok(NttManagerError::ZeroAmount))
    );
}

/// `quote_transfer` normalizes an amount to the lesser of local/peer decimals,
/// returning `(trimmed, dust)`: trimming down sheds dust (7->6), equal decimals
/// pass through, and a wider peer is capped at the local precision (7->9).
#[test]
fn quote_transfer_trims_and_reports_dust() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, 1, 1000, 3600);

    let peer = BytesN::from_array(&env, &[1u8; 32]);
    client.set_peer(&2, &peer, &6, &u64::MAX);
    client.set_peer(&3, &peer, &7, &u64::MAX);
    client.set_peer(&4, &peer, &9, &u64::MAX);

    // Local token has 7 decimals (SAC default).
    assert_eq!(client.quote_transfer(&1005, &2), (100, 5)); // trim 7->6, dust 5
    assert_eq!(client.quote_transfer(&1005, &3), (1005, 0)); // equal decimals
    assert_eq!(client.quote_transfer(&1005, &4), (1005, 0)); // peer wider, capped at local
}

/// `quote_delivery_price` returns one fee per enabled transceiver in registry
/// order, isolating a failing transceiver to `fee: None` rather than failing
/// the whole quote.
#[test]
fn quote_delivery_price_reports_each_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, 1, 1000, 3600);

    let ok_tx = add_transceiver(&env, &client, 500, false);
    let failing_tx = add_transceiver(&env, &client, 0, true);

    let fees = client.quote_delivery_price(&2);
    assert_eq!(fees.len(), 2);

    let first = fees.get(0).unwrap();
    assert_eq!(first.transceiver, ok_tx);
    assert_eq!(first.fee, Some(500));

    let second = fees.get(1).unwrap();
    assert_eq!(second.transceiver, failing_tx);
    assert_eq!(second.fee, None);
}
