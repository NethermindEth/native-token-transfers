#![cfg(test)]

mod common;
#[path = "common/token.rs"]
mod token;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::{setup_manager, setup_manager_with_token};
use soroban_ntt_client::{
    Mode, NttManagerError, NttManagerMessage, OutboundTransferCancelled, OutboundTransferQueued,
    OutboundTransferRateLimited, TransferSent, TrimmedAmount,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, BytesN, Env, Event,
};
use token::{MockToken, MockTokenClient};
use transceiver::{add_transceiver, MockTransceiverClient};

const OUR_CHAIN: u32 = 2;
const DEST_CHAIN: u32 = 6;

/// `token_decimals` reads the token on every call rather than a value cached
/// at construction, so it reports a change made after deployment.
#[test]
fn token_decimals_reads_token_live() {
    let env = Env::default();
    let token = env.register(MockToken, (7u32,));
    let (_, client) =
        setup_manager_with_token(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600, &token);

    env.as_contract(&token, || {
        env.storage().instance().set(&symbol_short!("dec"), &9u32)
    });

    assert_eq!(client.token_decimals(), 9);
}

/// Amounts are normalized to at most 8 decimals (`MAX_DECIMALS`): on an
/// 18-decimal token the sub-precision remainder is dust that stays with the
/// sender, only the trimmed amount is custodied, and the trimmed value (at 8
/// decimals) is what gets dispatched.
#[test]
fn transfer_retains_sub_precision_dust() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (18u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &[0xAA; 32]), &8, &u64::MAX);
    let sender = Address::generate(&env);

    let scale = 10_000_000_000i128; // 10^(18 - 8)
    let amount = 250 * scale + 42; // 42 is below the trim precision -> dust
    MockTokenClient::new(&env, &token).mint(&sender, &amount);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    client.transfer(&sender, &amount, &DEST_CHAIN, &recipient, &false);

    let mock = MockTokenClient::new(&env, &token);
    assert_eq!(mock.balance(&sender), 42); // dust retained
    assert_eq!(mock.balance(&client.address), 250 * scale); // only the trimmed amount is locked
    assert_eq!(client.get_outbound_capacity(), u64::MAX - 250); // rate-limited at 8 decimals

    let sent = MockTransceiverClient::new(&env, &transceiver).last_sent().unwrap();
    let decoded = NttManagerMessage::from_bytes(&env, &sent.manager_payload).unwrap();
    assert_eq!(decoded.payload.amount, TrimmedAmount::new(250, 8).unwrap());
}

/// An amount whose trimmed or rate-limit value would exceed `u64` is rejected
/// with `AmountOverflow` before any tokens are taken.
#[test]
fn transfer_rejects_amount_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    add_transceiver(&env, &client, 0, false);
    client.set_peer(&DEST_CHAIN, &BytesN::from_array(&env, &[0xAA; 32]), &1, &u64::MAX);
    let sender = Address::generate(&env);
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    // Local 7 decimals, peer 1 -> trim scale 10^6; this trims to u64::MAX + 1.
    let huge = (u64::MAX as i128 + 1) * 1_000_000;
    assert_eq!(
        client.try_transfer(&sender, &huge, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::AmountOverflow))
    );

    // This trims to u64::MAX at 1 decimal, which overflows at the 7 local decimals.
    let wide = u64::MAX as i128 * 1_000_000;
    assert_eq!(
        client.try_transfer(&sender, &wide, &DEST_CHAIN, &recipient, &false),
        Err(Ok(NttManagerError::AmountOverflow))
    );
    assert_eq!(
        client.try_quote_transfer(&wide, &DEST_CHAIN),
        Err(Ok(NttManagerError::AmountOverflow))
    );
}

/// A queued transfer's events (`OutboundTransferRateLimited` with the capacity,
/// then `OutboundTransferCancelled` or `TransferSent`) report the queued amount
/// in the token's decimals, as on EVM, not at the 8 rate-limit or 6 wire
/// decimals. 100 tokens of an 18-decimal token is 10^20, more than a `u64`
/// holds, and the 7 units of dust stay with the sender.
#[test]
fn queued_transfer_events_use_token_decimals() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (18u32,));
    // An outbound limit of 10 tokens at 8 decimals.
    let (_, client) =
        setup_manager_with_token(&env, Mode::Locking, OUR_CHAIN, 1_000_000_000, 3600, &token);
    add_transceiver(&env, &client, 0, false);
    client.set_peer(
        &DEST_CHAIN,
        &BytesN::from_array(&env, &[0xAA; 32]),
        &6,
        &u64::MAX,
    );
    let sender = Address::generate(&env);
    let amount = 100 * 10i128.pow(18);
    MockTokenClient::new(&env, &token).mint(&sender, &(amount + 7));
    let recipient = BytesN::from_array(&env, &[0x22; 32]);

    let queued = client.transfer(&sender, &(amount + 7), &DEST_CHAIN, &recipient, &true);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            OutboundTransferRateLimited {
                sender: sender.clone(),
                sequence: queued.sequence,
                amount,
                current_capacity: 10 * 10i128.pow(18),
            }
            .to_xdr(&env, &client.address),
            OutboundTransferQueued {
                queue_sequence: queued.sequence
            }
            .to_xdr(&env, &client.address),
        ]
    );

    client.cancel_queued_transfer(&sender, &queued.sequence);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![OutboundTransferCancelled {
            sequence: queued.sequence,
            recipient: sender.clone(),
            amount,
        }
        .to_xdr(&env, &client.address)]
    );
    assert_eq!(
        MockTokenClient::new(&env, &token).balance(&sender),
        amount + 7
    );

    let queued = client.transfer(&sender, &(amount + 7), &DEST_CHAIN, &recipient, &true);
    let item = client.get_outbound_queue_item(&queued.sequence).unwrap();
    env.ledger().set_timestamp(item.release_timestamp);
    let sent = client.complete_queued_transfer(&queued.sequence);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![TransferSent {
            recipient_chain: DEST_CHAIN,
            digest: sent.digest,
            recipient,
            amount,
            fee: 0,
            msg_sequence: sent.sequence,
        }
        .to_xdr(&env, &client.address)]
    );
}
