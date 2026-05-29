#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::setup_manager;
use messages::ntt_message;
use soroban_ntt_client::{
    Mode, NttManagerError, ThresholdChanged, TransceiverAdded, TransceiverRemoved,
};
use soroban_sdk::{testutils::Events as _, BytesN, Env, Event};
use transceiver::{add_transceiver, MockTransceiver};

const OUR_CHAIN: u32 = 2;
const SRC_CHAIN: u32 = 6;

/// Registering the first transceiver assigns index 0 and auto-sets threshold 1;
/// a second takes index 1; re-enabling an active one fails; removing disables it
/// (index retained) and re-adding re-enables it at the same index.
#[test]
fn transceiver_registry_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    let first = add_transceiver(&env, &client, 0, false);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![TransceiverAdded {
            transceiver: first.clone(),
            transceivers_num: 1,
            threshold: 1,
        }
        .to_xdr(&env, &client.address)]
    );
    let info = client.get_transceiver_info(&0).unwrap();
    assert_eq!(info.address, first);
    assert!(info.enabled);
    assert_eq!(info.index, 0);
    assert_eq!(client.get_transceiver_count(), 1);
    assert_eq!(client.get_enabled_bitmap(), 0b1);
    assert_eq!(client.get_threshold(), 1);

    add_transceiver(&env, &client, 0, false);
    assert_eq!(client.get_transceiver_count(), 2);
    assert_eq!(client.get_enabled_bitmap(), 0b11);

    assert_eq!(
        client.try_set_transceiver(&first),
        Err(Ok(NttManagerError::TransceiverAlreadyEnabled))
    );

    client.remove_transceiver(&first);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![TransceiverRemoved {
            transceiver: first.clone(),
            threshold: 1,
        }
        .to_xdr(&env, &client.address)]
    );
    assert!(!client.get_transceiver_info(&0).unwrap().enabled);
    assert_eq!(client.get_enabled_bitmap(), 0b10);
    assert_eq!(client.get_transceiver_count(), 2); // index is permanent

    assert_eq!(client.set_transceiver(&first), 0); // re-enabled at the same index
    assert!(client.get_transceiver_info(&0).unwrap().enabled);
    assert_eq!(client.get_enabled_bitmap(), 0b11);
    assert_eq!(client.get_transceiver_count(), 2);
}

/// The only enabled transceiver cannot be disabled, which would strand the
/// manager (`CannotDisableLastTransceiver`).
#[test]
fn cannot_disable_last_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let only = add_transceiver(&env, &client, 0, false);

    assert_eq!(
        client.try_remove_transceiver(&only),
        Err(Ok(NttManagerError::CannotDisableLastTransceiver))
    );
}

/// The registry caps at 64 transceivers (bitmap width); the 65th is rejected
/// (`MaxTransceiversReached`).
#[test]
fn max_transceivers_reached() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);

    for _ in 0..64 {
        add_transceiver(&env, &client, 0, false);
    }
    let extra = env.register(MockTransceiver, (0i128, false, false));
    assert_eq!(
        client.try_set_transceiver(&extra),
        Err(Ok(NttManagerError::MaxTransceiversReached))
    );
}

/// Threshold can move within `1..=enabled` (emitting `ThresholdChanged`), but
/// zero is rejected (`ZeroThreshold`) and exceeding the enabled count is too
/// (`ThresholdTooHigh`).
#[test]
fn threshold_transitions() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    add_transceiver(&env, &client, 0, false);
    add_transceiver(&env, &client, 0, false);

    client.set_threshold(&2);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![ThresholdChanged { old_threshold: 1, new_threshold: 2 }.to_xdr(&env, &client.address)]
    );
    assert_eq!(client.get_threshold(), 2);

    client.set_threshold(&1);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![ThresholdChanged { old_threshold: 2, new_threshold: 1 }.to_xdr(&env, &client.address)]
    );

    assert_eq!(
        client.try_set_threshold(&0),
        Err(Ok(NttManagerError::ZeroThreshold))
    );
    assert_eq!(
        client.try_set_threshold(&3),
        Err(Ok(NttManagerError::ThresholdTooHigh))
    );
}

/// Disabling a transceiver retroactively drops its attestation from the quorum
/// count, though the raw historical bit persists. The threshold auto-lowers to
/// the remaining enabled count (Soroban/Sui behavior).
#[test]
fn disabling_transceiver_drops_its_vote() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let first = add_transceiver(&env, &client, 0, false);
    add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);

    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    client.attestation_received(&first, &SRC_CHAIN, &source_manager, &payload);
    assert_eq!(client.message_attestations(&digest), 1);
    assert!(client.transceiver_attested_to_message(&digest, &0));
    assert!(!client.is_message_approved(&digest));

    client.remove_transceiver(&first);
    assert_eq!(client.message_attestations(&digest), 0); // disabled vote no longer counts
    assert!(client.transceiver_attested_to_message(&digest, &0)); // raw bit persists
    assert!(!client.is_message_approved(&digest));
}
