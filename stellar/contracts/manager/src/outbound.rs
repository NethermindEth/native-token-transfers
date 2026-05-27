//! Outbound transfer operations and queue management.
//!
//! This module handles the complete lifecycle of outbound cross-chain transfers:
//! - Immediate transfers that pass rate limiting
//! - Queued transfers that exceed rate limits
//! - Queue completion and cancellation (implemented in tasks 7.4)

use soroban_ntt_client::{
    address_to_bytes32, emit_outbound_transfer_cancelled, emit_outbound_transfer_queued,
    emit_outbound_transfer_rate_limited, emit_transfer_sent, flatten_call, sequence_to_message_id,
    NativeTokenTransfer, NttManagerError, NttManagerMessage, TransceiverClient, TrimmedAmount,
};
use soroban_sdk::{Address, Bytes, BytesN, Env};

use crate::{
    peers::{get_peer, refill_inbound},
    rate_limit::{consume_or_delay_outbound, RateLimitResult},
    state::{OutboundQueuedTransfer, TransferResult},
    storage::{InstanceStorage, OutboundQueueEntry},
    token_ops::{custody_tokens, get_token_decimals, release_tokens},
    transceivers::{get_enabled_bitmap, get_enabled_transceivers},
};

/// Sends a transfer message to all enabled transceivers.
///
/// Creates an NTT message, computes its digest, and dispatches it to each
/// enabled transceiver for cross-chain delivery. Uses `use_message_sequence`
/// to assign a unique sequence number. Returns the sequence and digest for
/// tracking and verification.
#[allow(clippy::too_many_arguments)]
pub fn send_transfer(
    env: &Env,
    sender: &Address,
    amount: &TrimmedAmount,
    recipient_chain: u32,
    recipient_ntt_manager: &BytesN<32>,
    recipient: &BytesN<32>,
    source_token: &BytesN<32>,
    additional_payload: &Option<Bytes>,
) -> Result<(u64, BytesN<32>), NttManagerError> {
    let storage = InstanceStorage::new(env);
    let sequence = storage.use_sequence();
    let message_id = sequence_to_message_id(env, sequence);
    let sender_bytes = address_to_bytes32(sender);

    let ntt_message = NttManagerMessage {
        id: message_id,
        sender: sender_bytes,
        payload: NativeTokenTransfer {
            amount: *amount,
            source_token: source_token.clone(),
            to: recipient.clone(),
            to_chain: recipient_chain,
            additional_payload: additional_payload.clone(),
        },
    };

    let our_chain_id = storage.chain_id()?;

    let digest = ntt_message.compute_digest(env, our_chain_id as u16)?;
    let payload = ntt_message.to_bytes(env)?;

    let transceivers = get_enabled_transceivers(env)?;
    if transceivers.is_empty() {
        return Err(NttManagerError::NoEnabledTransceivers);
    }

    for transceiver in transceivers.iter() {
        flatten_call(
            TransceiverClient::new(env, &transceiver).try_send_message(
                &recipient_chain,
                recipient_ntt_manager,
                &payload,
            ),
            NttManagerError::TransceiverCallFailed,
        )?;
    }

    emit_transfer_sent(env, recipient, amount.amount, 0, recipient_chain, sequence, &digest);

    Ok((sequence, digest))
}

/// Core transfer logic with rate limiting and queuing support.
///
/// Validates the transfer amount and recipient, normalizes decimals, and takes
/// custody of tokens. Checks the rate limiter and either sends immediately or
/// queues the transfer based on `should_queue`. If queueing is disabled and the
/// rate limit is exceeded, returns tokens and fails with `TransferExceedsRateLimit`.
///
/// # Errors
/// - `ZeroAmount` if amount is zero or negative
/// - `InvalidRecipient` if recipient is all zeros
/// - `PeerNotFound` if no peer registered for recipient chain
/// - `NoEnabledTransceivers` if no transceivers are enabled
/// - `TransferExceedsRateLimit` if rate limited and `should_queue` is false
pub fn transfer_internal(
    env: &Env,
    sender: &Address,
    amount: i128,
    recipient_chain: u32,
    recipient: &BytesN<32>,
    should_queue: bool,
    additional_payload: Option<Bytes>,
) -> Result<TransferResult, NttManagerError> {
    if amount <= 0 {
        return Err(NttManagerError::ZeroAmount);
    }

    let zero = BytesN::from_array(env, &[0u8; 32]);
    if *recipient == zero {
        return Err(NttManagerError::InvalidRecipient);
    }

    let peer = get_peer(env, recipient_chain).ok_or(NttManagerError::PeerNotFound)?;

    if get_enabled_bitmap(env).is_empty() {
        return Err(NttManagerError::NoEnabledTransceivers);
    }

    let storage = InstanceStorage::new(env);
    let token = storage.token()?;
    let our_decimals = get_token_decimals(env)?;

    let mut transfer_amount = amount as u128;
    let trimmed = TrimmedAmount::remove_dust(
        &mut transfer_amount,
        our_decimals as u8,
        peer.token_decimals as u8,
    )?;

    custody_tokens(env, sender, transfer_amount as i128)?;

    let rate_result = consume_or_delay_outbound(env, trimmed.amount);

    let source_token = address_to_bytes32(&token);

    match rate_result {
        RateLimitResult::Consumed => {
            let (sequence, digest) = send_transfer(
                env,
                sender,
                &trimmed,
                recipient_chain,
                &peer.address,
                recipient,
                &source_token,
                &additional_payload,
            )?;

            refill_inbound(env, recipient_chain, trimmed.amount);

            Ok(TransferResult::immediate(sequence, digest))
        }
        RateLimitResult::Delayed(release_timestamp) => {
            if !should_queue {
                release_tokens(env, sender, transfer_amount as i128)?;
                return Err(NttManagerError::TransferExceedsRateLimit);
            }

            let sequence = storage.use_sequence();
            let current_capacity = storage
                .outbound_rate_limit()
                .capacity_at(env, storage.rate_limit_duration());
            emit_outbound_transfer_rate_limited(
                env,
                sender,
                sequence,
                trimmed.amount,
                current_capacity,
            );

            let queued = OutboundQueuedTransfer {
                sender: sender.clone(),
                amount: trimmed,
                recipient_chain,
                recipient_ntt_manager: peer.address.clone(),
                recipient: recipient.clone(),
                source_token: source_token.clone(),
                release_timestamp,
                additional_payload: additional_payload.clone(),
            };

            OutboundQueueEntry::new(env, sequence).set(&queued);
            emit_outbound_transfer_queued(env, sequence);

            let message_id = sequence_to_message_id(env, sequence);
            let sender_bytes = address_to_bytes32(sender);
            let ntt_message = NttManagerMessage {
                id: message_id,
                sender: sender_bytes,
                payload: NativeTokenTransfer {
                    amount: trimmed,
                    source_token,
                    to: recipient.clone(),
                    to_chain: recipient_chain,
                    additional_payload,
                },
            };
            let our_chain_id = storage.chain_id()?;
            let digest = ntt_message.compute_digest(env, our_chain_id as u16)?;

            Ok(TransferResult::queued(sequence, digest))
        }
    }
}

/// Completes a queued outbound transfer after its release timestamp.
///
/// Anyone can call this once `release_timestamp` is reached. Removes the transfer
/// from the queue and sends the message to transceivers. The rate limit check is
/// skipped because the delay period has already been served.
///
/// The returned sequence number is a *new* sequence (not the original queue sequence).
///
/// # Errors
/// - `TransferNotQueued` if no transfer exists for the given sequence
/// - `TransferNotReleasable` if current time is before release timestamp
pub fn complete_outbound_queued_transfer(
    env: &Env,
    sequence: u64,
) -> Result<TransferResult, NttManagerError> {
    let entry = OutboundQueueEntry::new(env, sequence);
    let queued = entry.get_or_err()?;

    let now = env.ledger().timestamp();
    if now < queued.release_timestamp {
        return Err(NttManagerError::TransferNotReleasable);
    }

    entry.remove();

    // Rate limit check is intentionally skipped here.
    // The user already served their delay in the queue, so we proceed directly.
    // This matches EVM behavior in NttManager.sol:completeOutboundQueuedTransfer

    let (new_sequence, digest) = send_transfer(
        env,
        &queued.sender,
        &queued.amount,
        queued.recipient_chain,
        &queued.recipient_ntt_manager,
        &queued.recipient,
        &queued.source_token,
        &queued.additional_payload,
    )?;

    refill_inbound(env, queued.recipient_chain, queued.amount.amount);

    Ok(TransferResult::immediate(new_sequence, digest))
}

/// Cancels a queued outbound transfer and refunds tokens to the sender.
///
/// Only the original sender can cancel their queued transfer. Removes the transfer
/// from persistent storage and releases the tokens back to the sender. The refund
/// amount is calculated by expanding the trimmed amount back to the token's decimals.
///
/// # Errors
/// - `TransferNotQueued` if no transfer exists for the given sequence
/// - `CancellerNotSender` if caller is not the original sender
pub fn cancel_outbound_queued_transfer(
    env: &Env,
    sender: &Address,
    sequence: u64,
) -> Result<(), NttManagerError> {
    let entry = OutboundQueueEntry::new(env, sequence);
    let queued = entry.get_or_err()?;

    if queued.sender != *sender {
        return Err(NttManagerError::CancellerNotSender);
    }

    entry.remove();

    let token_decimals = get_token_decimals(env)?;
    let refund_amount = queued.amount.untrim(token_decimals as u8) as i128;
    let cancelled_amount = queued.amount.amount;

    release_tokens(env, sender, refund_amount)?;
    emit_outbound_transfer_cancelled(env, sequence, sender, cancelled_amount);

    Ok(())
}
