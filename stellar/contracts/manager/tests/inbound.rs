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
    bytes32_to_address, InboundTransferQueued, MessageAttestedTo, Mode, NttManagerError,
    TransferRedeemed,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Bytes, BytesN, Env, Event,
};
use stellar_ntt_manager::ManagerContractClient;
use token::{MockToken, MockTokenClient};
use transceiver::add_transceiver;

const OUR_CHAIN: u32 = 2;
const SRC_CHAIN: u32 = 6;
const OTHER_CHAIN: u32 = 9;

/// Locking manager (SAC) with one enabled transceiver (threshold 1) and a peer
/// registered for SRC_CHAIN. For error-path tests that never release tokens.
fn wired<'a>(env: &Env) -> (ManagerContractClient<'a>, Address, BytesN<32>) {
    let (_, _, client) = setup_manager(env, Mode::Locking, OUR_CHAIN, u64::MAX, 3600);
    let transceiver = add_transceiver(env, &client, 0, false);
    let source_manager = BytesN::from_array(env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);
    (client, transceiver, source_manager)
}

/// Only registered, enabled transceivers may attest: an unknown caller is
/// rejected (`TransceiverNotRegistered`), and one that was removed stays indexed
/// but barred (`TransceiverNotEnabled`).
#[test]
fn attestation_received_rejects_unregistered_or_disabled_transceiver() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, registered, source_manager) = wired(&env);
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
    let (client, transceiver, source_manager) = wired(&env);
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
    let (client, transceiver, source_manager) = wired(&env);

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
    let (client, first, source_manager) = wired(&env);
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
    let (client, first, source_manager) = wired(&env);
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
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let first = add_transceiver(&env, &client, 0, false);
    let second = add_transceiver(&env, &client, 0, false);
    client.set_threshold(&2);
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &u64::MAX);

    let recipient_bytes = BytesN::from_array(&env, &[0x22; 32]);
    let recipient = bytes32_to_address(&env, &recipient_bytes);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_bytes, OUR_CHAIN, SRC_CHAIN);

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

/// An inbound transfer over the peer's rate limit is queued (not released) yet
/// still marked executed for replay protection, emitting `InboundTransferQueued`.
#[test]
fn attestation_received_queues_when_rate_limited() {
    let env = Env::default();
    env.mock_all_auths();
    let token = env.register(MockToken, (7u32,));
    let (_, client) = setup_manager_with_token(&env, Mode::Burning, OUR_CHAIN, u64::MAX, 3600, &token);
    let transceiver = add_transceiver(&env, &client, 0, false);
    let source_manager = BytesN::from_array(&env, &[0x11; 32]);
    // Inbound capacity below the transfer amount forces queuing.
    client.set_peer(&SRC_CHAIN, &source_manager, &7, &50);

    let recipient_bytes = BytesN::from_array(&env, &[0x22; 32]);
    let recipient = bytes32_to_address(&env, &recipient_bytes);
    let (payload, digest) = ntt_message(&env, 100, 7, &recipient_bytes, OUR_CHAIN, SRC_CHAIN);

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
    let (client, first, source_manager) = wired(&env);
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
