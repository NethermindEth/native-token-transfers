#![cfg(test)]

mod common;
#[path = "common/messages.rs"]
mod messages;
#[path = "common/token.rs"]
mod token;
#[path = "common/transceiver.rs"]
mod transceiver;

use common::{setup_manager, setup_manager_with_token};
use messages::ntt_message;
use soroban_ntt_client::{
    bytes32_to_address, MessageAlreadyExecuted, MessageAttestedTo, Mode, NttManagerError,
    TransferRedeemed,
};
use soroban_sdk::{
    testutils::{Events as _, Ledger as _},
    BytesN, Env, Event,
};
use token::{MockToken, MockTokenClient};
use transceiver::add_transceiver;

const OUR_CHAIN: u32 = 2;
const SRC_CHAIN: u32 = 6;

/// A queued inbound transfer cannot be completed before its release window
/// (`TransferNotReleasable`); after advancing the ledger it releases tokens and
/// emits `TransferRedeemed`, and a second completion fails (`TransferNotQueued`).
#[test]
fn complete_inbound_transfer_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &50); // low limit forces queuing

    let recipient_bytes = BytesN::from_array(&env, &[0x22; 32]);
    let recipient = bytes32_to_address(&env, &recipient_bytes);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_bytes, OUR_CHAIN, SRC_CHAIN);

    assert!(client
        .attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload)
        .queued);

    assert_eq!(
        client.try_complete_inbound_transfer(&digest),
        Err(Ok(NttManagerError::TransferNotReleasable))
    );

    let release_at = client.get_inbound_queue_item(&digest).unwrap().release_timestamp;
    env.ledger().set_timestamp(release_at);

    client.complete_inbound_transfer(&digest);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![TransferRedeemed { digest: digest.clone() }.to_xdr(&env, &client.address)]
    );
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100);

    assert_eq!(
        client.try_complete_inbound_transfer(&digest),
        Err(Ok(NttManagerError::TransferNotQueued))
    );
}

/// A second transceiver attesting an already-executed digest is idempotent:
/// it records its vote and emits `MessageAlreadyExecuted` without releasing
/// tokens again.
#[test]
fn attestation_on_executed_digest_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let first = add_transceiver(&env, &client, 0, false);
    let second = add_transceiver(&env, &client, 0, false); // threshold stays 1
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);

    let recipient_bytes = BytesN::from_array(&env, &[0x22; 32]);
    let recipient = bytes32_to_address(&env, &recipient_bytes);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_bytes, OUR_CHAIN, SRC_CHAIN);

    assert!(client
        .attestation_received(&first, &SRC_CHAIN, &source_manager, &payload)
        .executed);
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100);

    let replay = client.attestation_received(&second, &SRC_CHAIN, &source_manager, &payload);
    assert!(replay.executed);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            MessageAttestedTo {
                digest: digest.clone(),
                transceiver: second,
                index: 1,
            }
            .to_xdr(&env, &client.address),
            MessageAlreadyExecuted {
                source_ntt_manager: source_manager,
                msg_hash: digest,
            }
            .to_xdr(&env, &client.address),
        ]
    );
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100); // not re-minted
}

/// `execute_msg` on a digest with no recorded attestation is rejected
/// (`TransferNotApproved`).
#[test]
fn execute_msg_rejects_unapproved_message() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, _) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    assert_eq!(
        client.try_execute_msg(&SRC_CHAIN, &source_manager, &payload),
        Err(Ok(NttManagerError::TransferNotApproved))
    );
}

/// `execute_msg` on an already-executed digest is idempotent: it emits
/// `MessageAlreadyExecuted` and does not release tokens again.
#[test]
fn execute_msg_on_executed_digest_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false); // threshold 1
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);

    let recipient_bytes = BytesN::from_array(&env, &[0x22; 32]);
    let recipient = bytes32_to_address(&env, &recipient_bytes);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_bytes, OUR_CHAIN, SRC_CHAIN);

    assert!(client
        .attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload)
        .executed);

    let replay = client.execute_msg(&SRC_CHAIN, &source_manager, &payload);
    assert!(replay.executed);
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![MessageAlreadyExecuted {
            source_ntt_manager: source_manager,
            msg_hash: digest,
        }
        .to_xdr(&env, &client.address)]
    );
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100); // not re-minted
}
