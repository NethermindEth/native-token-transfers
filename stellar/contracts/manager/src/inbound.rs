//! Inbound transfer operations and attestation processing.
//!
//! This module handles incoming cross-chain transfers:
//! - Attestation collection from multiple transceivers
//! - Threshold verification before releasing tokens
//! - Rate-limited queuing for large inbound transfers
//! - Queue completion after rate limit delay expires
//! - Manual execution of approved but unexecuted messages

use soroban_sdk::{Address, Bytes, BytesN, Env};

use crate::errors::NttManagerError;
use crate::messages::NttManagerMessage;
use crate::peers::{consume_or_queue_inbound, verify_peer};
use crate::rate_limit::{refill_outbound, RateLimitResult};
use crate::state::{
    address_to_bytes32, resolve_stellar_address, AddressResolution, AttestationResult,
    InboundQueuedTransfer, PendingAddressTransfer,
};
use crate::storage::{AttestationEntry, InboundQueueEntry, InstanceStorage, PendingAddressEntry};
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

    let digest = ntt_message.compute_digest(env, source_chain as u16)?;

    let attestation_entry = AttestationEntry::new(env, digest.clone());
    let mut attestation = attestation_entry.get_or_default();

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    let attester_bit = 1u64 << transceiver_index;
    if attestation.attested_transceivers & attester_bit != 0 {
        return Err(NttManagerError::TransceiverAlreadyAttested);
    }

    attestation.attested_transceivers |= attester_bit;
    attestation_entry.set(&attestation);

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
/// Validates the destination chain, resolves the recipient address by probing
/// the ledger for both Account and Contract interpretations, and untrims the
/// amount to local token decimals.
///
/// If the recipient exists on-chain, proceeds with rate limiting and token release
/// (or queuing). If the recipient cannot be resolved (neither Account nor Contract
/// exists), parks the transfer as a `PendingAddressTransfer` — the real recipient
/// must later call `claim_pending_transfer` to receive tokens.
///
/// On successful release, refills the outbound rate limit by the transfer amount
/// (backflow mechanism to maintain bidirectional capacity).
fn execute_inbound_transfer(
    env: &Env,
    source_chain: u32,
    message: &NttManagerMessage,
    digest: &BytesN<32>,
) -> Result<AttestationResult, NttManagerError> {
    let storage = InstanceStorage::new(env);
    let transfer = &message.payload;

    let our_chain_id = storage.chain_id()?;

    if transfer.to_chain != our_chain_id {
        return Err(NttManagerError::InvalidTargetChain);
    }

    let our_decimals = get_token_decimals(env)?;
    let release_amount = transfer.amount.untrim(our_decimals as u8) as i128;

    match resolve_stellar_address(env, &transfer.to) {
        AddressResolution::Resolved(recipient) => execute_resolved_transfer(
            env,
            source_chain,
            &recipient,
            release_amount,
            transfer.amount.amount,
            digest,
        ),
        AddressResolution::NotFound => {
            // Recipient doesn't exist on ledger yet. Park the transfer for later claim.
            // Rate limit is NOT consumed — deferred until claim time.
            // Attestation is NOT marked executed — will be marked at claim time.
            let pending = PendingAddressTransfer {
                recipient_bytes: transfer.to.clone(),
                amount: release_amount,
                trimmed_amount: transfer.amount.amount,
                source_chain,
            };

            PendingAddressEntry::new(env, digest.clone()).set(&pending);

            Ok(AttestationResult {
                approved: true,
                executed: false,
                queued: false,
            })
        }
    }
}

/// Completes a resolved transfer with rate limiting and token release.
///
/// Shared by `execute_inbound_transfer` (normal path) and `claim_pending_transfer`
/// (deferred path). Checks the inbound rate limit and either releases tokens
/// immediately or queues the transfer.
fn execute_resolved_transfer(
    env: &Env,
    source_chain: u32,
    recipient: &Address,
    release_amount: i128,
    trimmed_amount: u64,
    digest: &BytesN<32>,
) -> Result<AttestationResult, NttManagerError> {
    let rate_result = consume_or_queue_inbound(env, source_chain, trimmed_amount)?;

    match rate_result {
        RateLimitResult::Consumed => {
            let attestation_entry = AttestationEntry::new(env, digest.clone());
            let mut attestation = attestation_entry.get_or_default();
            attestation.executed = true;
            attestation_entry.set(&attestation);

            release_tokens(env, recipient, release_amount)?;

            refill_outbound(env, trimmed_amount);

            Ok(AttestationResult {
                approved: true,
                executed: true,
                queued: false,
            })
        }
        RateLimitResult::Delayed(release_timestamp) => {
            let queued = InboundQueuedTransfer {
                recipient: recipient.clone(),
                amount: release_amount,
                trimmed_amount,
                release_timestamp,
            };

            InboundQueueEntry::new(env, digest.clone()).set(&queued);

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
/// Also refills the outbound rate limit using the stored trimmed amount (backflow).
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
    let queue_entry = InboundQueueEntry::new(env, digest.clone());
    let queued = queue_entry.get_or_err()?;

    let now = env.ledger().timestamp();
    if now < queued.release_timestamp {
        return Err(NttManagerError::TransferNotReleasable);
    }

    let attestation_entry = AttestationEntry::new(env, digest.clone());
    let mut attestation = attestation_entry
        .get()
        .ok_or(NttManagerError::TransferNotApproved)?;

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    attestation.executed = true;
    attestation_entry.set(&attestation);

    queue_entry.remove();

    release_tokens(env, &queued.recipient, queued.amount)?;
    refill_outbound(env, queued.trimmed_amount);

    Ok(())
}

/// Manually executes an approved message that hasn't been executed yet.
///
/// Allows execution of transfers where the attestation threshold was met but
/// automatic execution didn't occur (e.g., due to rate limiting or transaction
/// failure). Only attestations from currently enabled transceivers count toward
/// the threshold.
///
/// This is permissionless: anyone can call it to help complete pending transfers.
///
/// # Errors
/// - `PeerNotFound` or `InvalidPeer` if source doesn't match registered peer
/// - `TransferNotApproved` if threshold not met with currently enabled transceivers
/// - `TransferAlreadyRedeemed` if tokens were already released
pub fn execute_msg_internal(
    env: &Env,
    source_chain: u32,
    source_ntt_manager: &BytesN<32>,
    payload: &Bytes,
) -> Result<AttestationResult, NttManagerError> {
    verify_peer(env, source_chain, source_ntt_manager)?;

    let ntt_message = NttManagerMessage::from_bytes(env, payload)?;
    let digest = ntt_message.compute_digest(env, source_chain as u16)?;

    let attestation = AttestationEntry::new(env, digest.clone())
        .get()
        .ok_or(NttManagerError::TransferNotApproved)?;

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    let enabled_bitmap = get_enabled_bitmap(env);
    let valid_attestations = attestation.attested_transceivers & enabled_bitmap.raw();
    let attestation_count = valid_attestations.count_ones() as u32;
    let threshold = get_threshold(env);

    if attestation_count < threshold {
        return Err(NttManagerError::TransferNotApproved);
    }

    execute_inbound_transfer(env, source_chain, &ntt_message, &digest)
}

/// Claims a pending address transfer after the recipient's address is live on-chain.
///
/// When an inbound transfer targets a 32-byte address that doesn't exist on Stellar
/// at execution time (neither as Account nor Contract), the transfer is parked as a
/// `PendingAddressTransfer`. The real recipient calls this function to claim it.
///
/// The caller must authenticate and their `address_to_bytes32` representation must
/// match the original 32-byte recipient in the wire message.
///
/// Rate limiting and token release are deferred to this point — capacity is consumed
/// only when the recipient actually claims.
///
/// # Errors
/// - `PendingTransferNotFound` if no pending transfer exists for the digest
/// - `PendingTransferRecipientMismatch` if caller's bytes32 doesn't match
/// - `TransferAlreadyRedeemed` if attestation was already executed
pub fn claim_pending_transfer_internal(
    env: &Env,
    recipient: &Address,
    digest: &BytesN<32>,
) -> Result<AttestationResult, NttManagerError> {
    let pending_entry = PendingAddressEntry::new(env, digest.clone());
    let pending = pending_entry.get_or_err()?;

    // Verify the claimant's bytes32 matches the original wire recipient.
    let recipient_bytes = address_to_bytes32(env, recipient);
    if recipient_bytes != pending.recipient_bytes {
        return Err(NttManagerError::PendingTransferRecipientMismatch);
    }

    // Verify attestation exists and hasn't been executed.
    let attestation = AttestationEntry::new(env, digest.clone())
        .get()
        .ok_or(NttManagerError::TransferNotApproved)?;

    if attestation.executed {
        return Err(NttManagerError::TransferAlreadyRedeemed);
    }

    // Remove the pending entry before proceeding.
    pending_entry.remove();

    // Now execute with rate limiting, same as a normally resolved transfer.
    execute_resolved_transfer(
        env,
        pending.source_chain,
        recipient,
        pending.amount,
        pending.trimmed_amount,
        digest,
    )
}
