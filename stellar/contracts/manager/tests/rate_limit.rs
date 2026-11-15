#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;
#[path = "common/recipient.rs"]
mod recipient;
#[path = "common/token.rs"]
mod token;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::{setup_manager, setup_manager_with_token};
use recipient::record_recipient;
use messages::ntt_message;
use soroban_ntt_client::{Mode, NttManagerError};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::StellarAssetClient,
    Address, BytesN, Env,
};
use token::{MockToken, MockTokenClient};
use transceiver::add_transceiver;

const OUR_CHAIN: u32 = 2;
const PEER_CHAIN: u32 = 6;

/// Outbound and inbound rate limits are coupled: each direction consumes its own
/// capacity and *refills the opposite* (backflow), so sustained two-way traffic
/// can't deadlock on a one-directional limit. The peer has 6 decimals to the
/// token's 7, and both buckets still move by local amounts, not the 10x smaller
/// wire amounts.
#[test]
fn circular_rate_limit_backflow() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, 1000, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false); // threshold 1
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &6, &1000);
    let sender = Address::generate(&env);
    MockTokenClient::new(&env, &token).mint(&sender, &1000);

    assert_eq!(client.get_outbound_capacity(), 1000);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(1000));

    // Outbound consumes outbound capacity; the inbound top-up is capped (already full).
    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    client.transfer(&sender, &400, &PEER_CHAIN, &recipient, &false);
    assert_eq!(client.get_outbound_capacity(), 600);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(1000));

    // Inbound release consumes inbound capacity and refills outbound (backflow).
    let inbound_recipient = Address::generate(&env);
    let inbound_recipient_hash = record_recipient(&env, &client, &inbound_recipient);
    let (payload, _) = ntt_message(&env, 30, 6, &inbound_recipient_hash, OUR_CHAIN, PEER_CHAIN);
    client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(700));
    assert_eq!(client.get_outbound_capacity(), 900); // 600 + 300 backflow

    // A further outbound transfer now refills the consumed inbound capacity.
    client.transfer(&sender, &200, &PEER_CHAIN, &recipient, &false);
    assert_eq!(client.get_outbound_capacity(), 700); // 900 - 200
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(900)); // 700 + 200 backflow
}

/// Consumed outbound capacity refills linearly over the rate-limit duration:
/// half the duration restores half the limit.
#[test]
fn outbound_capacity_refills_over_time() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, token, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, 1000, 3600);
    add_transceiver(&env, &client, 0, false);
    client.set_peer(&PEER_CHAIN, &BytesN::from_array(&env, &[0x11; 32]), &7, &u64::MAX);
    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    client.transfer(&sender, &800, &PEER_CHAIN, &recipient, &false);
    assert_eq!(client.get_outbound_capacity(), 200);

    // Advance half the 3600s duration -> refill 1000 * 1800 / 3600 = 500.
    env.ledger().set_timestamp(1800);
    assert_eq!(client.get_outbound_capacity(), 700);
}

/// Every queued outbound transfer waits the full duration, however small its
/// deficit, and queueing leaves the bucket untouched.
#[test]
fn queued_outbound_waits_full_duration() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, token, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, 300, 3600);
    add_transceiver(&env, &client, 0, false);
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &7, &u64::MAX);
    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &903);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    let sequences: [u64; 3] = std::array::from_fn(|_| {
        client
            .transfer(&sender, &301, &PEER_CHAIN, &recipient, &true)
            .sequence
    });
    for sequence in &sequences {
        let queued = client.get_outbound_queue_item(sequence).unwrap();
        assert_eq!(queued.release_timestamp, 3600);
    }
    assert_eq!(client.get_outbound_capacity(), 300);

    env.ledger().set_timestamp(3599);
    for sequence in &sequences {
        assert_eq!(
            client.try_complete_queued_transfer(sequence),
            Err(Ok(NttManagerError::TransferNotReleasable))
        );
    }
    env.ledger().set_timestamp(3600);
    for sequence in &sequences {
        client.complete_queued_transfer(sequence);
    }
}

/// Every queued inbound transfer waits the full duration, however small its
/// deficit, and queueing leaves the peer's bucket untouched.
#[test]
fn queued_inbound_waits_full_duration() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, 1000, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &7, &300);

    // Distinct recipients give the three messages distinct digests.
    let digests: [BytesN<32>; 3] = std::array::from_fn(|_| {
        let recipient = record_recipient(&env, &client, &Address::generate(&env));
        let (payload, digest) = ntt_message(&env, 301, 7, &recipient, OUR_CHAIN, PEER_CHAIN);
        client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
        digest
    });
    for digest in &digests {
        let queued = client.get_inbound_queue_item(digest).unwrap();
        assert_eq!(queued.release_timestamp, 3600);
    }
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(300));

    env.ledger().set_timestamp(3599);
    for digest in &digests {
        assert_eq!(
            client.try_complete_inbound_transfer(digest),
            Err(Ok(NttManagerError::TransferNotReleasable))
        );
    }
    env.ledger().set_timestamp(3600);
    for digest in &digests {
        client.complete_inbound_transfer(digest);
    }
}

/// Limits count `min(8, local decimals)` units, so a 6-decimal peer of a
/// 7-decimal token does not loosen them: 10_000 local units is 1000 on the wire
/// but 10_000 against a limit of 1000, in both directions.
#[test]
fn lower_decimal_peer_consumes_local_units() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, 1000, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &6, &1000);
    let sender = Address::generate(&env);
    MockTokenClient::new(&env, &token).mint(&sender, &10_000);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    assert_eq!(
        client.try_transfer(&sender, &10_000, &PEER_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::TransferExceedsRateLimit))
    );

    let to = record_recipient(&env, &client, &Address::generate(&env));
    let (payload, digest) = ntt_message(&env, 1000, 6, &to, OUR_CHAIN, PEER_CHAIN);
    client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
    let queued = client.get_inbound_queue_item(&digest).unwrap();
    assert_eq!(queued.trimmed_amount, 10_000);
}

/// Completing a queued outbound transfer leaves the peer's inbound bucket alone:
/// the transfer never consumed outbound capacity, so it has nothing to backflow.
#[test]
fn completing_queued_outbound_does_not_refill_inbound() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, 1000, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &7, &1000);
    let sender = Address::generate(&env);
    MockTokenClient::new(&env, &token).mint(&sender, &1500);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let queued = client.transfer(&sender, &1500, &PEER_CHAIN, &recipient, &true);
    let item = client.get_outbound_queue_item(&queued.sequence).unwrap();
    env.ledger().set_timestamp(item.release_timestamp);

    let to = record_recipient(&env, &client, &Address::generate(&env));
    let (payload, _) = ntt_message(&env, 600, 7, &to, OUR_CHAIN, PEER_CHAIN);
    client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(400));

    client.complete_queued_transfer(&queued.sequence);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(400));
}

/// Completing a queued inbound transfer leaves the outbound bucket alone: the
/// transfer never consumed inbound capacity, so it has nothing to backflow.
#[test]
fn completing_queued_inbound_does_not_refill_outbound() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, 1000, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let peer_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&PEER_CHAIN, &peer_manager, &7, &1000);
    let sender = Address::generate(&env);
    MockTokenClient::new(&env, &token).mint(&sender, &600);

    let to = record_recipient(&env, &client, &Address::generate(&env));
    let (payload, digest) = ntt_message(&env, 1500, 7, &to, OUR_CHAIN, PEER_CHAIN);
    client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
    let item = client.get_inbound_queue_item(&digest).unwrap();
    env.ledger().set_timestamp(item.release_timestamp);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    client.transfer(&sender, &600, &PEER_CHAIN, &recipient, &false);
    assert_eq!(client.get_outbound_capacity(), 400);

    client.complete_inbound_transfer(&digest);
    assert_eq!(client.get_outbound_capacity(), 400);
}
