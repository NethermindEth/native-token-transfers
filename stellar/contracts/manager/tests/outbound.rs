#![cfg(test)]

mod common;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::setup_manager;
use soroban_ntt_client::{
    sequence_to_message_id, Mode, NttManagerError, NttManagerMessage, OutboundTransferCancelled,
    OutboundTransferQueued, OutboundTransferRateLimited, TransferSent, TrimmedAmount,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, Event,
};
use stellar_ntt_manager::ManagerContractClient;
use transceiver::{add_transceiver, MockTransceiver, MockTransceiverClient};
use wormhole_soroban_client::hash_address;

const OUR_CHAIN: u32 = 2;
const DEST_CHAIN: u32 = 6;
const PEER_MANAGER: [u8; 32] = [0x11; 32];
const RECIPIENT: [u8; 32] = [0x22; 32];

/// Manager over a 7-decimal SAC with one enabled transceiver and a peer at
/// DEST_CHAIN, plus a sender funded with 1000 units. Returns owner, client,
/// token, transceiver, and sender.
fn outbound_setup<'a>(
    env: &Env,
    mode: Mode,
    outbound_limit: u64,
) -> (Address, ManagerContractClient<'a>, Address, Address, Address) {
    let (owner, token, client) = setup_manager(env, mode, OUR_CHAIN, outbound_limit, 3600);
    let transceiver = add_transceiver(env, &client, 0, false);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(env, &PEER_MANAGER), &7, &u64::MAX);
    let sender = Address::generate(env);
    StellarAssetClient::new(env, &token).mint(&sender, &1000);
    (owner, client, token, transceiver, sender)
}

/// A locking transfer moves tokens into the manager and dispatches a correctly
/// built NTT message to each transceiver, returning an immediate result and
/// emitting `TransferSent`.
#[test]
fn transfer_locks_tokens_and_dispatches() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, token, transceiver, sender) = outbound_setup(&env, Mode::Locking, u64::MAX);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let result = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &false);
    assert!(!result.queued);
    assert_eq!(result.sequence, 1);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![TransferSent {
            recipient_chain: DEST_CHAIN,
            digest: result.digest.clone(),
            recipient: recipient.clone(),
            amount: 100,
            fee: 0,
            msg_sequence: 1,
        }
        .to_xdr(&env, &client.address)]
    );

    // The manager dispatched the message the peer chain expects.
    let sent = MockTransceiverClient::new(&env, &transceiver).last_sent().unwrap();
    assert_eq!(sent.recipient_chain, DEST_CHAIN);
    assert_eq!(sent.recipient_manager, BytesN::from_array(&env, &PEER_MANAGER));
    let decoded = NttManagerMessage::from_bytes(&env, &sent.manager_payload).unwrap();
    assert_eq!(decoded.id, sequence_to_message_id(&env, 1));
    assert_eq!(decoded.sender, hash_address(&env, &sender));
    assert_eq!(decoded.payload.to, recipient);
    assert_eq!(decoded.payload.to_chain, DEST_CHAIN);
    assert_eq!(decoded.payload.amount, TrimmedAmount::new(100, 7).unwrap());
    assert_eq!(decoded.payload.source_token, hash_address(&env, &token));

    let mock = TokenClient::new(&env, &token);
    assert_eq!(mock.balance(&sender), 900);
    assert_eq!(mock.balance(&client.address), 100);
}

/// `transfer_with_payload` threads the caller's additional payload into the
/// dispatched NTT message. A dropped or unserialized payload would silently
/// deliver the transfer without the data the destination contract expects.
#[test]
fn transfer_with_payload_dispatches_additional_payload() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, _, transceiver, sender) = outbound_setup(&env, Mode::Locking, u64::MAX);
    let recipient = BytesN::from_array(&env, &RECIPIENT);
    let payload = Bytes::from_array(&env, &[0xDE, 0xAD, 0xBE, 0xEF]);

    client.transfer_with_payload(&sender, &100, &DEST_CHAIN, &recipient, &false, &payload);

    let sent = MockTransceiverClient::new(&env, &transceiver).last_sent().unwrap();
    let decoded = NttManagerMessage::from_bytes(&env, &sent.manager_payload).unwrap();
    assert_eq!(decoded.payload.additional_payload, Some(payload));
}

/// A burning transfer reduces the sender's balance without the manager holding
/// custody (supply leaves this chain), and still dispatches the message.
#[test]
fn transfer_burns_supply_and_dispatches() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, token, transceiver, sender) = outbound_setup(&env, Mode::Burning, u64::MAX);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let result = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &false);
    assert!(!result.queued);

    let mock = TokenClient::new(&env, &token);
    assert_eq!(mock.balance(&sender), 900);
    assert_eq!(mock.balance(&client.address), 0);
    assert!(MockTransceiverClient::new(&env, &transceiver).last_sent().is_some());
}

/// `transfer` rejects a zero amount (`ZeroAmount`), the zero recipient
/// (`InvalidRecipient`), and a chain with no registered peer (`PeerNotFound`).
#[test]
fn transfer_rejects_invalid_inputs() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let sender = Address::generate(&env);
    let recipient = BytesN::from_array(&env, &RECIPIENT);
    let zero = BytesN::from_array(&env, &[0u8; 32]);

    assert_eq!(
        client.try_transfer(&sender, &0, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::ZeroAmount))
    );
    assert_eq!(
        client.try_transfer(&sender, &100, &DEST_CHAIN, &zero, &false),
        Err(Ok(NttManagerError::InvalidRecipient))
    );
    assert_eq!(
        client.try_transfer(&sender, &100, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::PeerNotFound))
    );
}

/// A transfer to a configured peer with no enabled transceiver is rejected
/// (`NoEnabledTransceivers`) — there is no path to deliver it.
#[test]
fn transfer_requires_enabled_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER_MANAGER), &7, &u64::MAX);
    let sender = Address::generate(&env);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    assert_eq!(
        client.try_transfer(&sender, &100, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::NoEnabledTransceivers))
    );
}

/// When a transceiver's `send_message` fails, the manager surfaces
/// `TransceiverCallFailed` and the whole transfer reverts (tokens not taken).
#[test]
fn transfer_propagates_transceiver_send_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, token, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let failing = env.register(MockTransceiver, (0i128, false, true));
    client.set_transceiver(&failing);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &PEER_MANAGER), &7, &u64::MAX);
    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    assert_eq!(
        client.try_transfer(&sender, &100, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::TransceiverCallFailed))
    );
    assert_eq!(TokenClient::new(&env, &token).balance(&sender), 1000);
}

/// An over-limit transfer with `should_queue` is custodied and queued (not
/// dispatched), emitting `OutboundTransferRateLimited` then
/// `OutboundTransferQueued`.
#[test]
fn transfer_queues_when_rate_limited() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, token, transceiver, sender) = outbound_setup(&env, Mode::Locking, 50);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let result = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &true);
    assert!(result.queued);
    assert_eq!(result.sequence, 1);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            OutboundTransferRateLimited {
                sender: sender.clone(),
                sequence: 1,
                amount: 100,
                current_capacity: 50,
            }
            .to_xdr(&env, &client.address),
            OutboundTransferQueued { queue_sequence: 1 }.to_xdr(&env, &client.address),
        ]
    );

    let queued = client.get_outbound_queue_item(&1).unwrap();
    assert_eq!(queued.amount.amount, 100);
    assert_eq!(queued.sender, sender);
    assert!(MockTransceiverClient::new(&env, &transceiver).last_sent().is_none());
    assert_eq!(TokenClient::new(&env, &token).balance(&sender), 900);
}

/// An over-limit transfer without `should_queue` is rejected with
/// `TransferExceedsRateLimit`.
#[test]
fn transfer_rejects_over_limit_without_queue() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, _, _, sender) = outbound_setup(&env, Mode::Locking, 50);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    assert_eq!(
        client.try_transfer(&sender, &100, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::TransferExceedsRateLimit))
    );
}

/// A queued transfer cannot complete before its release window
/// (`TransferNotReleasable`); after advancing the ledger it dispatches, and a
/// second completion fails because the entry is consumed (`TransferNotQueued`).
#[test]
fn complete_queued_transfer_releases_after_window() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, _, transceiver, sender) = outbound_setup(&env, Mode::Locking, 50);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let queued = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &true);
    let sequence = queued.sequence;

    assert_eq!(
        client.try_complete_queued_transfer(&sequence),
        Err(Ok(NttManagerError::TransferNotReleasable))
    );

    let release_at = client.get_outbound_queue_item(&sequence).unwrap().release_timestamp;
    env.ledger().set_timestamp(release_at);

    let completed = client.complete_queued_transfer(&sequence);
    assert!(!completed.queued);
    assert!(MockTransceiverClient::new(&env, &transceiver).last_sent().is_some());

    assert_eq!(
        client.try_complete_queued_transfer(&sequence),
        Err(Ok(NttManagerError::TransferNotQueued))
    );
}

/// Only the original sender can cancel a queued transfer (`CancellerNotSender`
/// otherwise); cancelling refunds the sender, emits `OutboundTransferCancelled`,
/// and consumes the entry (`TransferNotQueued` on retry).
#[test]
fn cancel_queued_transfer_refunds_sender() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, token, _, sender) = outbound_setup(&env, Mode::Locking, 50);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let queued = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &true);
    let sequence = queued.sequence;
    assert_eq!(TokenClient::new(&env, &token).balance(&sender), 900);

    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_cancel_queued_transfer(&stranger, &sequence),
        Err(Ok(NttManagerError::CancellerNotSender))
    );

    client.cancel_queued_transfer(&sender, &sequence);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![OutboundTransferCancelled {
            sequence,
            recipient: sender.clone(),
            amount: 100,
        }
        .to_xdr(&env, &client.address)]
    );

    assert_eq!(TokenClient::new(&env, &token).balance(&sender), 1000);
    assert_eq!(
        client.try_cancel_queued_transfer(&sender, &sequence),
        Err(Ok(NttManagerError::TransferNotQueued))
    );
}

/// `complete_queued_transfer` is gated by `#[when_not_paused]`: while paused it
/// trips `EnforcedPause` (1000), surfaced as a host contract error.
#[test]
fn complete_queued_transfer_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, client, _, _, sender) = outbound_setup(&env, Mode::Locking, 50);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let queued = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &true);
    client.pause(&owner);

    assert_eq!(
        client.try_complete_queued_transfer(&queued.sequence),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}

/// Each transfer gets a distinct sequence, so two otherwise-identical transfers
/// produce distinct digests and cannot collide on the receiving side.
#[test]
fn repeated_transfers_produce_distinct_digests() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client, _, _, sender) = outbound_setup(&env, Mode::Locking, u64::MAX);
    let recipient = BytesN::from_array(&env, &RECIPIENT);

    let first = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &false);
    let second = client.transfer(&sender, &100, &DEST_CHAIN, &recipient, &false);

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert_ne!(first.digest, second.digest);
}
