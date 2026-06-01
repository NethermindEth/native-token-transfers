#![cfg(test)]

mod common;

use common::setup_transceiver;
use soroban_ntt_client::{PeerSet, TransceiverError};
use soroban_sdk::{testutils::Events as _, BytesN, Env, Event};

const DEST_CHAIN: u32 = 6;
const TOO_LARGE: u32 = u16::MAX as u32 + 1;

/// `set_peer` persists the peer (enabled) and emits `PeerSet { chain_id, peer }`.
/// Storage drift makes the next inbound VAA from that chain miss its peer; event
/// drift blinds off-chain observers.
#[test]
fn set_peer_registers_and_emits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);
    let emitter = BytesN::from_array(&env, &[0xAA; 32]);

    transceiver.set_peer(&DEST_CHAIN, &emitter);
    assert_eq!(
        env.events().all().filter_by_contract(&transceiver.address),
        std::vec![PeerSet { chain_id: DEST_CHAIN, peer_contract: emitter.clone() }
            .to_xdr(&env, &transceiver.address)]
    );

    assert_eq!(transceiver.get_peer(&DEST_CHAIN), Some(emitter));
    assert!(transceiver.is_peer_enabled(&DEST_CHAIN));
}

/// `set_peer` validates its arguments and is one-shot: chain 0, the zero emitter,
/// an out-of-range chain, and re-registering an existing chain are each rejected.
/// The one-shot guard is the security boundary — a second write could swap the
/// emitter a chain's VAAs authenticate against.
#[test]
fn set_peer_rejects_invalid_params() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);
    let emitter = BytesN::from_array(&env, &[0xAA; 32]);
    let zeros = BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        transceiver.try_set_peer(&0, &emitter),
        Err(Ok(TransceiverError::InvalidPeerChainIdZero))
    );
    assert_eq!(
        transceiver.try_set_peer(&DEST_CHAIN, &zeros),
        Err(Ok(TransceiverError::InvalidPeerZeroAddress))
    );
    assert_eq!(
        transceiver.try_set_peer(&TOO_LARGE, &emitter),
        Err(Ok(TransceiverError::ChainIdTooLarge))
    );

    transceiver.set_peer(&DEST_CHAIN, &emitter);
    assert_eq!(
        transceiver.try_set_peer(&DEST_CHAIN, &emitter),
        Err(Ok(TransceiverError::PeerAlreadySet))
    );
}

/// `set_peer_enabled` flips the enabled flag without dropping the registration,
/// and rejects an unregistered chain with `PeerNotFound` rather than persisting a
/// half-state a typo'd chain id would otherwise leave behind.
#[test]
fn set_peer_enabled_flips_or_errors_on_missing() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);

    assert_eq!(
        transceiver.try_set_peer_enabled(&DEST_CHAIN, &false),
        Err(Ok(TransceiverError::PeerNotFound))
    );

    let emitter = BytesN::from_array(&env, &[0xAA; 32]);
    transceiver.set_peer(&DEST_CHAIN, &emitter);

    transceiver.set_peer_enabled(&DEST_CHAIN, &false);
    assert!(!transceiver.is_peer_enabled(&DEST_CHAIN));
    assert_eq!(transceiver.get_peer(&DEST_CHAIN), Some(emitter)); // still registered, just disabled

    transceiver.set_peer_enabled(&DEST_CHAIN, &true);
    assert!(transceiver.is_peer_enabled(&DEST_CHAIN));
}

/// `set_peer_enabled` validates the chain id through its own `validate_chain_id`
/// call, independent of `set_peer`'s — an out-of-range chain is rejected with
/// `ChainIdTooLarge` rather than reaching storage.
#[test]
fn set_peer_enabled_rejects_chain_id_too_large() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, transceiver, _, _) = setup_transceiver(&env);

    assert_eq!(
        transceiver.try_set_peer_enabled(&TOO_LARGE, &true),
        Err(Ok(TransceiverError::ChainIdTooLarge))
    );
}
