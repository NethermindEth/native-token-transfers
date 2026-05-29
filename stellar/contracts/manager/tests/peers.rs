#![cfg(test)]

mod common;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::setup_manager;
use soroban_ntt_client::{Mode, NttManagerError, PeerUpdated};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    token::StellarAssetClient,
    Address, BytesN, Env, Event,
};
use transceiver::add_transceiver;

const OUR_CHAIN: u32 = 2;
const DEST_CHAIN: u32 = 6;

/// `set_peer` validates its arguments: chain 0, this manager's own chain, the
/// zero address, and out-of-range decimals are each rejected.
#[test]
fn set_peer_rejects_invalid_params() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let peer = BytesN::from_array(&env, &[0xAA; 32]);
    let zeros = BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client.try_set_peer(&0, &peer, &7, &u64::MAX),
        Err(Ok(NttManagerError::InvalidPeerChainIdZero))
    );
    assert_eq!(
        client.try_set_peer(&OUR_CHAIN, &peer, &7, &u64::MAX),
        Err(Ok(NttManagerError::InvalidPeerSameChainId))
    );
    assert_eq!(
        client.try_set_peer(&DEST_CHAIN, &zeros, &7, &u64::MAX),
        Err(Ok(NttManagerError::InvalidPeerZeroAddress))
    );
    assert_eq!(
        client.try_set_peer(&DEST_CHAIN, &peer, &0, &u64::MAX),
        Err(Ok(NttManagerError::InvalidPeerDecimals))
    );
    assert_eq!(
        client.try_set_peer(&DEST_CHAIN, &peer, &19, &u64::MAX),
        Err(Ok(NttManagerError::InvalidPeerDecimals))
    );
}

/// Registering a peer stores it and emits `PeerUpdated` (old fields zeroed);
/// overwriting an existing peer reports its previous address and decimals.
#[test]
fn set_peer_registers_and_reports_previous() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let peer = BytesN::from_array(&env, &[0xAA; 32]);

    client.set_peer(&DEST_CHAIN, &peer, &7, &u64::MAX);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![PeerUpdated {
            chain_id: DEST_CHAIN,
            old_peer_contract: BytesN::from_array(&env, &[0u8; 32]),
            old_peer_decimals: 0,
            peer_contract: peer.clone(),
            peer_decimals: 7,
        }
        .to_xdr(&env, &client.address)]
    );
    let stored = client.get_peer(&DEST_CHAIN).unwrap();
    assert_eq!(stored.address, peer);
    assert_eq!(stored.token_decimals, 7);

    let new_peer = BytesN::from_array(&env, &[0xBB; 32]);
    client.set_peer(&DEST_CHAIN, &new_peer, &9, &u64::MAX);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![PeerUpdated {
            chain_id: DEST_CHAIN,
            old_peer_contract: peer,
            old_peer_decimals: 7,
            peer_contract: new_peer,
            peer_decimals: 9,
        }
        .to_xdr(&env, &client.address)]
    );
}

/// Changing the outbound limit preserves the already-consumed amount: after
/// spending 300 of 1000, raising the limit to 2000 leaves capacity 1700, and
/// lowering it to 500 leaves 200 (300 still consumed).
#[test]
fn set_outbound_limit_preserves_consumed_capacity() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, token, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, 1000, 3600);
    add_transceiver(&env, &client, 0, false);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &[0xAA; 32]), &7, &u64::MAX);
    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    client.transfer(&sender, &300, &DEST_CHAIN, &recipient, &false);
    assert_eq!(client.get_outbound_capacity(), 700);

    client.set_outbound_limit(&2000);
    assert_eq!(client.get_outbound_capacity(), 1700);

    client.set_outbound_limit(&500);
    assert_eq!(client.get_outbound_capacity(), 200);
}

/// `set_inbound_limit` requires a registered peer (`PeerNotFound` otherwise) and
/// updates that peer's inbound limit.
#[test]
fn set_inbound_limit_updates_peer_or_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    assert_eq!(
        client.try_set_inbound_limit(&DEST_CHAIN, &500),
        Err(Ok(NttManagerError::PeerNotFound))
    );

    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &[0xAA; 32]), &7, &1000);
    assert_eq!(client.get_inbound_limit_params(&DEST_CHAIN).unwrap().limit, 1000);

    client.set_inbound_limit(&DEST_CHAIN, &500);
    assert_eq!(client.get_inbound_limit_params(&DEST_CHAIN).unwrap().limit, 500);
}
