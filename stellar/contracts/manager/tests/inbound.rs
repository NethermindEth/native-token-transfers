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
    InboundTransferQueued, MessageAlreadyExecuted, MessageAttestedTo, Mode, NttManagerError,
    TransferRedeemed,
};
use soroban_sdk::{
    address_payload::AddressPayload,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Bytes, BytesN, Env, Event,
};
use stellar_ntt_manager::ManagerContractClient;
use token::{MockToken, MockTokenClient};
use transceiver::add_transceiver;
use wormhole_soroban_client::hash_address;

const OUR_CHAIN: u32 = 2;
const SRC_CHAIN: u32 = 6;
const OTHER_CHAIN: u32 = 9;

/// Locking manager (SAC) with one enabled transceiver (threshold 1) and a peer
/// registered for SRC_CHAIN. For error-path tests that never release tokens.
fn locking_inbound<'a>(env: &Env) -> (ManagerContractClient<'a>, Address, BytesN<32>) {
    let (_, _, client) = setup_manager(env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let transceiver = add_transceiver(env, &client, 0, false);
    let source_manager = BytesN::from_array(env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);
    (client, transceiver, source_manager)
}

/// Burning manager over a 7-decimal mock token with one enabled transceiver and
/// a peer at SRC_CHAIN (capacity `inbound_limit`). For tests that release tokens.
/// Returns client, token, transceiver, and source-manager bytes.
fn burning_inbound<'a>(
    env: &Env,
    inbound_limit: u64,
) -> (ManagerContractClient<'a>, Address, Address, BytesN<32>) {
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(env, &client, 0, false);
    let source_manager = BytesN::from_array(env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &inbound_limit);
    (client, token, transceiver, source_manager)
}

/// Only registered, enabled transceivers may attest: an unknown caller is
/// rejected (`TransceiverNotRegistered`), and one that was removed stays indexed
/// but barred (`TransceiverNotEnabled`).
#[test]
fn attestation_received_rejects_unregistered_or_disabled_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, registered, source_manager) = locking_inbound(&env);
    // A second transceiver keeps the enabled set non-empty when `registered` is removed.
    add_transceiver(&env, &client, 0, false);
    let dummy = Bytes::new(&env);

    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_attestation_received(&stranger, &SRC_CHAIN, &source_manager, &dummy),
        Err(Ok(NttManagerError::TransceiverNotRegistered))
    );

    client.remove_transceiver(&registered);
    assert_eq!(
        client.try_attestation_received(&registered, &SRC_CHAIN, &source_manager, &dummy),
        Err(Ok(NttManagerError::TransceiverNotEnabled))
    );
}

/// Inbound messages are authenticated against the registered peer: an
/// unconfigured source chain gives `PeerNotFound`, and a source-manager address
/// that isn't the registered peer gives `InvalidPeer` (anti-spoofing).
#[test]
fn attestation_received_rejects_unknown_or_mismatched_peer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, transceiver, source_manager) = locking_inbound(&env);
    let dummy = Bytes::new(&env);

    assert_eq!(
        client.try_attestation_received(&transceiver, &OTHER_CHAIN, &source_manager, &dummy),
        Err(Ok(NttManagerError::PeerNotFound))
    );

    let impostor = BytesN::from_array(&env, &[0x99; 32]);
    assert_eq!(
        client.try_attestation_received(&transceiver, &SRC_CHAIN, &impostor, &dummy),
        Err(Ok(NttManagerError::InvalidPeer))
    );
}

/// A message whose `to_chain` isn't this manager's chain is refused
/// (`InvalidTargetChain`), so a misrouted transfer cannot release tokens here.
#[test]
fn attestation_received_rejects_wrong_target_chain() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, transceiver, source_manager) = locking_inbound(&env);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, _) = ntt_message(&env, 100, 7, &recipient, OTHER_CHAIN, SRC_CHAIN);

    assert_eq!(
        client.try_attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload),
        Err(Ok(NttManagerError::InvalidTargetChain))
    );
}

/// A single attestation under a 2-of-N threshold records the vote (bitmap bit +
/// `MessageAttestedTo`) without approving or executing the transfer.
#[test]
fn attestation_received_records_vote_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, first, source_manager) = locking_inbound(&env);
    add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    let result = client.attestation_received(&first, &SRC_CHAIN, &source_manager, &payload);
    assert!(!result.approved && !result.executed && !result.queued);
    // `env.events()` reflects only the last invocation, so assert before any query call.
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![MessageAttestedTo {
            digest: digest.clone(),
            transceiver: first,
            index: 0,
        }
        .to_xdr(&env, &client.address)]
    );

    assert_eq!(client.message_attestations(&digest), 1);
    assert!(client.transceiver_attested_to_message(&digest, &0));
    assert!(!client.transceiver_attested_to_message(&digest, &1));
    assert!(!client.is_message_approved(&digest));
    assert!(!client.is_message_executed(&digest));
}

/// One transceiver cannot reach quorum alone: a repeat attestation for the same
/// digest is rejected (`TransceiverAlreadyAttested`).
#[test]
fn attestation_received_rejects_double_vote() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, first, source_manager) = locking_inbound(&env);
    add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, _) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    client.attestation_received(&first, &SRC_CHAIN, &source_manager, &payload);
    assert_eq!(
        client.try_attestation_received(&first, &SRC_CHAIN, &source_manager, &payload),
        Err(Ok(NttManagerError::TransceiverAlreadyAttested))
    );
}

/// Reaching the threshold releases the transfer: tokens are minted to the
/// recipient, the digest is marked executed, and `MessageAttestedTo` then
/// `TransferRedeemed` are emitted.
#[test]
fn attestation_received_executes_at_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, first, source_manager) = burning_inbound(&env, u64::MAX);
    let second = add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);

    let recipient = Address::generate(&env);
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

    let pending = client.attestation_received(&first, &SRC_CHAIN, &source_manager, &payload);
    assert!(!pending.approved);
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 0);

    let result = client.attestation_received(&second, &SRC_CHAIN, &source_manager, &payload);
    assert!(result.approved && result.executed && !result.queued);
    // `env.events()` reflects only the last invocation, so assert before any query call.
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            MessageAttestedTo {
                digest: digest.clone(),
                transceiver: second,
                index: 1,
            }
            .to_xdr(&env, &client.address),
            TransferRedeemed {
                digest: digest.clone(),
            }
            .to_xdr(&env, &client.address),
        ]
    );

    assert!(client.is_message_executed(&digest));
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100);
}

/// Locking-mode release unlocks the manager's held tokens to the recipient (a
/// `transfer` out), mirroring burning-mode mint but moving custody rather than
/// creating supply. Uses a `G…` account recipient, so the suite exercises the
/// full inbound release for both address kinds (the burning path uses a `C…`
/// contract).
#[test]
fn attestation_releases_via_unlock_in_locking_mode() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false); // threshold 1
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);
    // The manager must hold tokens to unlock, as if locked by a prior outbound.
    MockTokenClient::new(&env, &token).mint(&client.address, &1000);

    let recipient = Address::from_payload(
        &env,
        AddressPayload::AccountIdPublicKeyEd25519(BytesN::from_array(&env, &[7u8; 32])),
    );
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

    assert!(client
        .attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload)
        .executed);
    assert!(client.is_message_executed(&digest));

    let mock = MockTokenClient::new(&env, &token);
    assert_eq!(mock.balance(&recipient), 100); // unlocked to the recipient
    assert_eq!(mock.balance(&client.address), 900); // released from the manager's custody
}

/// An inbound transfer over the peer's rate limit is queued (not released) yet
/// still marked executed for replay protection, emitting `InboundTransferQueued`.
#[test]
fn attestation_received_queues_when_rate_limited() {
    let env = Env::default();
    env.mock_all_auths();
    // Inbound capacity below the transfer amount forces queuing.
    let (client, token, transceiver, source_manager) = burning_inbound(&env, 50);

    let recipient = Address::generate(&env);
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

    let result = client.attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload);
    assert!(result.approved && !result.executed && result.queued);
    // `env.events()` reflects only the last invocation, so assert before any query call.
    assert_eq!(
        env.events().all().filter_by_contract(&client.address),
        std::vec![
            MessageAttestedTo {
                digest: digest.clone(),
                transceiver,
                index: 0,
            }
            .to_xdr(&env, &client.address),
            InboundTransferQueued {
                digest: digest.clone(),
            }
            .to_xdr(&env, &client.address),
        ]
    );

    let queued = client.get_inbound_queue_item(&digest).unwrap();
    assert_eq!(queued.amount, 100);
    assert_eq!(queued.recipient, recipient);
    // Marked executed to block replay, but tokens are not released yet.
    assert!(client.is_message_executed(&digest));
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 0);
}

/// The `#[when_not_paused]` gate blocks inbound processing while paused. The
/// error originates in OZ `PausableError` (EnforcedPause = 1000), outside
/// `NttManagerError`, so it surfaces as a host contract error.
#[test]
fn attestation_received_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (owner, _, client) = setup_manager(&env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);
    client.pause(&owner);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, _) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    assert_eq!(
        client.try_attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload),
        Err(Err(soroban_sdk::InvokeError::Contract(1000)))
    );
}

/// Attestations require the calling transceiver's authorization: with the auth
/// mock cleared, the call is rejected, so a third party cannot forge an
/// attestation. Threshold 2 ensures auth is the only possible failure.
#[test]
fn attestation_received_requires_transceiver_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, first, source_manager) = locking_inbound(&env);
    add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);

    let recipient = BytesN::from_array(&env, &[0x22; 32]);
    let (payload, _) = ntt_message(&env, 100, 7, &recipient, OUR_CHAIN, SRC_CHAIN);

    // Drop the blanket auth mock so the transceiver must explicitly authorize.
    env.mock_auths(&[]);
    assert!(client
        .try_attestation_received(&first, &SRC_CHAIN, &source_manager, &payload)
        .is_err());
}

/// A queued inbound transfer cannot be completed before its release window
/// (`TransferNotReleasable`); after advancing the ledger it releases tokens and
/// emits `TransferRedeemed`, and a second completion fails (`TransferNotQueued`).
#[test]
fn complete_inbound_transfer_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    // Low inbound limit forces queuing.
    let (client, token, transceiver, source_manager) = burning_inbound(&env, 50);

    let recipient = Address::generate(&env);
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

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

/// A second transceiver attesting an already-executed digest is idempotent: it
/// records its vote and emits `MessageAlreadyExecuted` without releasing tokens
/// again.
#[test]
fn attestation_on_executed_digest_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, first, source_manager) = burning_inbound(&env, u64::MAX);
    let second = add_transceiver(&env, &client, 0, false); // threshold stays 1

    let recipient = Address::generate(&env);
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

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
    let (client, token, transceiver, source_manager) = burning_inbound(&env, u64::MAX); // threshold 1

    let recipient = Address::generate(&env);
    client.record_address(&recipient);
    let recipient_hash = hash_address(&env, &recipient);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

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

/// An inbound transfer to an unregistered recipient fails with
/// `RecipientNotRegistered` and reverts the whole tx, so nothing is executed,
/// voted, or queued. After the recipient calls `record_address`, the same redeem
/// succeeds — the failure is retryable and no funds are stuck.
#[test]
fn redeem_unregistered_recipient_is_retryable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, token, transceiver, source_manager) = burning_inbound(&env, u64::MAX);

    let recipient = Address::generate(&env);
    let recipient_hash = hash_address(&env, &recipient); // intentionally not recorded
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_hash, OUR_CHAIN, SRC_CHAIN);

    assert_eq!(
        client.try_attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload),
        Err(Ok(NttManagerError::RecipientNotRegistered))
    );
    assert!(!client.is_message_executed(&digest));
    assert_eq!(client.message_attestations(&digest), 0);
    assert!(client.get_inbound_queue_item(&digest).is_none());
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 0);

    client.record_address(&recipient);
    assert!(client
        .attestation_received(&transceiver, &SRC_CHAIN, &source_manager, &payload)
        .executed);
    assert_eq!(MockTokenClient::new(&env, &token).balance(&recipient), 100);
}
