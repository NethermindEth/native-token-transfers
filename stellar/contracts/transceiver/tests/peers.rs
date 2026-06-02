#![cfg(test)]

mod common;

use common::setup;
use soroban_ntt_client::{PeerSet, TransceiverError};
use soroban_sdk::{testutils::Events as _, BytesN, Env, Event};

const PEER_CHAIN: u32 = 2;
const PEER_EMITTER: [u8; 32] = [0xCC; 32];
const TOO_LARGE: u32 = u16::MAX as u32 + 1;

/// `set_peer` persists the peer (enabled) and emits `PeerSet { chain_id, peer }`.
/// Storage drift makes the next inbound VAA from that chain miss its peer; event
/// drift blinds off-chain observers.
#[test]
fn set_peer_registers_and_emits() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let emitter = BytesN::from_array(&env, &PEER_EMITTER);

    t.set_peer(&PEER_CHAIN, &emitter);
    assert_eq!(
        env.events().all().filter_by_contract(&t.address),
        std::vec![PeerSet { chain_id: PEER_CHAIN, peer_contract: emitter.clone() }.to_xdr(&env, &t.address)]
    );

    assert_eq!(t.get_peer(&PEER_CHAIN), Some(emitter));
    assert!(t.is_peer_enabled(&PEER_CHAIN));
}

/// `set_peer` validates its arguments and is one-shot: chain 0, the zero emitter,
/// an out-of-range chain, and re-registering an existing chain are each rejected.
/// The one-shot guard is the security boundary — a second write could swap the
/// emitter a chain's VAAs authenticate against.
#[test]
fn set_peer_rejects_invalid_params() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);
    let emitter = BytesN::from_array(&env, &PEER_EMITTER);
    let zeros = BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(t.try_set_peer(&0, &emitter), Err(Ok(TransceiverError::InvalidPeerChainIdZero)));
    assert_eq!(t.try_set_peer(&PEER_CHAIN, &zeros), Err(Ok(TransceiverError::InvalidPeerZeroAddress)));
    assert_eq!(t.try_set_peer(&TOO_LARGE, &emitter), Err(Ok(TransceiverError::ChainIdTooLarge)));

    t.set_peer(&PEER_CHAIN, &emitter);
    assert_eq!(t.try_set_peer(&PEER_CHAIN, &emitter), Err(Ok(TransceiverError::PeerAlreadySet)));
}

/// `set_peer_enabled` flips the enabled flag without dropping the registration,
/// and rejects an unregistered chain with `PeerNotFound` rather than persisting a
/// half-state a typo'd chain id would otherwise leave behind.
#[test]
fn set_peer_enabled_flips_or_errors_on_missing() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);

    assert_eq!(t.try_set_peer_enabled(&PEER_CHAIN, &false), Err(Ok(TransceiverError::PeerNotFound)));

    let emitter = BytesN::from_array(&env, &PEER_EMITTER);
    t.set_peer(&PEER_CHAIN, &emitter);

    t.set_peer_enabled(&PEER_CHAIN, &false);
    assert!(!t.is_peer_enabled(&PEER_CHAIN));
    assert_eq!(t.get_peer(&PEER_CHAIN), Some(emitter)); // still registered, just disabled

    t.set_peer_enabled(&PEER_CHAIN, &true);
    assert!(t.is_peer_enabled(&PEER_CHAIN));
}

/// `set_peer_enabled` validates the chain id through its own `validate_chain_id`
/// call, independent of `set_peer`'s — an out-of-range chain is rejected with
/// `ChainIdTooLarge` rather than reaching storage.
#[test]
fn set_peer_enabled_rejects_chain_id_too_large() {
    let env = Env::default();
    env.mock_all_auths();
    let t = setup(&env);

    assert_eq!(t.try_set_peer_enabled(&TOO_LARGE, &true), Err(Ok(TransceiverError::ChainIdTooLarge)));
}
