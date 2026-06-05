#![cfg(test)]

mod common;
#[path = "common/token.rs"]
mod token;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::{setup_manager, setup_manager_with_token};
use soroban_ntt_client::{Mode, NttManagerError, NttManagerMessage, TrimmedAmount};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use token::{MockToken, MockTokenClient};
use transceiver::{add_transceiver, MockTransceiverClient};

const OUR_CHAIN: u32 = 2;
const DEST_CHAIN: u32 = 6;

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

    let sent = MockTransceiverClient::new(&env, &transceiver).last_sent().unwrap();
    let decoded = NttManagerMessage::from_bytes(&env, &sent.manager_payload).unwrap();
    assert_eq!(decoded.payload.amount, TrimmedAmount::new(250, 8).unwrap());
}

/// An amount whose trimmed value would exceed `u64` is rejected with
/// `AmountOverflow` before any tokens are taken.
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
}
