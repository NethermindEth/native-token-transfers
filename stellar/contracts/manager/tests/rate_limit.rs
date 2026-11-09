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
use soroban_ntt_client::{
    Mode, NttManagerError, OutboundTransferQueued, OutboundTransferRateLimited,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::StellarAssetClient,
    Address, BytesN, Env, Event,
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
    MockTokenClient::new(&env, &token).mint(&sender, &1500);

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

    // A queued transfer backflows on completion. Emptying the inbound bucket at
    // release time keeps its time-based refill out of the count.
    let queued = client.transfer(&sender, &900, &PEER_CHAIN, &recipient, &true);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            OutboundTransferRateLimited {
                sender: sender.clone(),
                sequence: queued.sequence,
                amount: 900,
                current_capacity: 700,
            }
            .to_xdr(&env, &client.address),
            OutboundTransferQueued {
                queue_sequence: queued.sequence
            }
            .to_xdr(&env, &client.address),
        ]
    );
    let item = client.get_outbound_queue_item(&queued.sequence).unwrap();
    env.ledger().set_timestamp(item.release_timestamp);
    let (payload, _) = ntt_message(&env, 100, 6, &inbound_recipient_hash, OUR_CHAIN, PEER_CHAIN);
    client.attestation_received(&transceiver, &PEER_CHAIN, &peer_manager, &payload);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(0));
    client.complete_queued_transfer(&queued.sequence);
    assert_eq!(client.get_inbound_capacity(&PEER_CHAIN), Some(900));
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
