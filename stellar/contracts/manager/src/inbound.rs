//! Inbound transfer operations and attestation processing.
//!
//! This module handles incoming cross-chain transfers:
//! - Attestation collection from multiple transceivers
//! - Threshold verification before releasing tokens
//! - Rate-limited queuing for large inbound transfers
//! - Queue completion after rate limit delay expires

use soroban_sdk::{Address, Bytes, BytesN, Env};

use crate::errors::NttManagerError;
use crate::messages::NttManagerMessage;
use crate::peers::{consume_or_queue_inbound, verify_peer};
use crate::rate_limit::{refill_outbound, RateLimitResult};
use crate::state::{
    bytes32_to_address, extend_persistent_ttl, AttestationInfo, AttestationResult, DataKey,
    InboundQueuedTransfer,
};
use crate::token_ops::{get_token_decimals, release_tokens};
use crate::transceivers::{
    get_enabled_bitmap, get_threshold, get_transceiver, get_transceiver_index,
};

/// Processes an attestation from a transceiver for an inbound message.
///
/// Verifies the transceiver is registered and enabled, validates the source peer,
/// parses the NTT message, and records the attestation in a bitmap. If the attestation
/// threshold is met, executes the transfer (releasing tokens or queuing if rate-limited).
///
/// Each transceiver can only attest once per message digest. The digest is computed
/// from the message contents and source chain ID for replay protection.
///
/// # Errors
/// - `TransceiverNotRegistered` if caller is not a registered transceiver
/// - `TransceiverNotEnabled` if the transceiver is disabled
/// - `PeerNotFound` or `InvalidPeer` if source doesn't match registered peer
/// - `TransferAlreadyRedeemed` if tokens were already released for this message
/// - `TransceiverAlreadyAttested` if this transceiver already attested
pub fn attestation_received_internal(
    env: &Env,
    transceiver: &Address,
    source_chain: u32,
    source_ntt_manager: &BytesN<32>,
    payload: &Bytes,
) -> Result<AttestationResult, NttManagerError> {
    let transceiver_index =
        get_transceiver_index(env, transceiver).ok_or(NttManagerError::TransceiverNotRegistered)?;

    let transceiver_info =
        get_transceiver(env, transceiver_index).ok_or(NttManagerError::TransceiverNotRegistered)?;

    if !transceiver_info.enabled {
        return Err(NttManagerError::TransceiverNotEnabled);
    }

    verify_peer(env, source_chain, source_ntt_manager)?;

    let ntt_message = NttManagerMessage::from_bytes(env, payload)?;

    let digest = ntt_message.compute_digest(env, source_chain as u16);

    let key = DataKey::Attestation(digest.clone());
    let mut attestation: AttestationInfo =
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(AttestationInfo {
                executed: false,
                attested_transceivers: 0,
            });

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    let attester_bit = 1u64 << transceiver_index;
    if attestation.attested_transceivers & attester_bit != 0 {
        return Err(NttManagerError::TransceiverAlreadyAttested);
    }

    attestation.attested_transceivers |= attester_bit;
    env.storage().persistent().set(&key, &attestation);
    extend_persistent_ttl(env, &key);

    let enabled_bitmap = get_enabled_bitmap(env);
    let valid_attestations = attestation.attested_transceivers & enabled_bitmap.raw();
    let attestation_count = valid_attestations.count_ones() as u32;
    let threshold = get_threshold(env);

    if attestation_count < threshold {
        return Ok(AttestationResult {
            approved: false,
            executed: false,
            queued: false,
        });
    }

    execute_inbound_transfer(env, source_chain, &ntt_message, &digest)
}

/// Executes a transfer after attestation threshold is met.
///
/// Validates the destination chain matches this contract, converts the recipient
/// address from bytes32, and untrims the amount to local token decimals. Checks
/// the per-chain inbound rate limit: if capacity exists, releases tokens immediately
/// and marks the attestation as executed; otherwise queues the transfer for later.
///
/// On successful release, refills the outbound rate limit by the transfer amount
/// (backflow mechanism to maintain bidirectional capacity).
fn execute_inbound_transfer(
    env: &Env,
    source_chain: u32,
    message: &NttManagerMessage,
    digest: &BytesN<32>,
) -> Result<AttestationResult, NttManagerError> {
    let transfer = &message.payload;

    let our_chain_id: u32 = env
        .storage()
        .instance()
        .get(&DataKey::ChainId)
        .ok_or(NttManagerError::NotInitialized)?;

    if transfer.to_chain != our_chain_id {
        return Err(NttManagerError::InvalidTargetChain);
    }

    let recipient = bytes32_to_address(env, &transfer.to);

    let our_decimals = get_token_decimals(env)?;
    let release_amount = transfer.amount.untrim(our_decimals as u8) as i128;

    let rate_result = consume_or_queue_inbound(env, source_chain, transfer.amount.amount)?;

    match rate_result {
        RateLimitResult::Consumed => {
            let key = DataKey::Attestation(digest.clone());
            let mut attestation: AttestationInfo = env.storage().persistent().get(&key).unwrap();
            attestation.executed = true;
            env.storage().persistent().set(&key, &attestation);

            release_tokens(env, &recipient, release_amount)?;

            refill_outbound(env, transfer.amount.amount);

            Ok(AttestationResult {
                approved: true,
                executed: true,
                queued: false,
            })
        }
        RateLimitResult::Delayed(release_timestamp) => {
            let queued = InboundQueuedTransfer {
                recipient,
                amount: release_amount,
                release_timestamp,
            };

            env.storage()
                .persistent()
                .set(&DataKey::InboundQueue(digest.clone()), &queued);
            extend_persistent_ttl(env, &DataKey::InboundQueue(digest.clone()));

            Ok(AttestationResult {
                approved: true,
                executed: false,
                queued: true,
            })
        }
    }
}

/// Completes a queued inbound transfer after its release timestamp.
///
/// Anyone can call this once `release_timestamp` is reached. Retrieves the queued
/// transfer by digest, verifies the delay period has passed, marks the attestation
/// as executed, removes the queue entry, and releases tokens to the recipient.
///
/// Also refills the outbound rate limit by the transfer amount (backflow). The amount
/// is converted back to trimmed form for the refill calculation.
///
/// # Errors
/// - `TransferNotQueued` if no queued transfer exists for the digest
/// - `TransferNotReleasable` if current time is before `release_timestamp`
/// - `TransferNotApproved` if attestation record is missing
/// - `TransferAlreadyRedeemed` if tokens were already released
pub fn complete_inbound_queued_transfer(
    env: &Env,
    digest: &BytesN<32>,
) -> Result<(), NttManagerError> {
    let queue_key = DataKey::InboundQueue(digest.clone());
    let queued: InboundQueuedTransfer = env
        .storage()
        .persistent()
        .get(&queue_key)
        .ok_or(NttManagerError::TransferNotQueued)?;

    let now = env.ledger().timestamp();
    if now < queued.release_timestamp {
        return Err(NttManagerError::TransferNotReleasable);
    }

    let attest_key = DataKey::Attestation(digest.clone());
    let mut attestation: AttestationInfo = env
        .storage()
        .persistent()
        .get(&attest_key)
        .ok_or(NttManagerError::TransferNotApproved)?;

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    attestation.executed = true;
    env.storage().persistent().set(&attest_key, &attestation);

    env.storage().persistent().remove(&queue_key);

    release_tokens(env, &queued.recipient, queued.amount)?;

    let decimals = get_token_decimals(env)?;
    let trimmed_amount = if decimals > 8 {
        (queued.amount as u128) / 10u128.pow((decimals - 8) as u32)
    } else {
        queued.amount as u128
    };
    refill_outbound(env, trimmed_amount as u64);

    Ok(())
}
