//! Outbound transfer operations and queue management.
//!
//! This module handles the complete lifecycle of outbound cross-chain transfers:
//! - Immediate transfers that pass rate limiting
//! - Queued transfers that exceed rate limits
//! - Queue completion and cancellation (implemented in tasks 7.4)

use soroban_sdk::{vec, Address, Bytes, BytesN, Env, IntoVal, Symbol};

use crate::errors::NttManagerError;
use crate::messages::{NativeTokenTransfer, NttManagerMessage, TrimmedAmount};
use crate::peers::{get_peer, refill_inbound};
use crate::rate_limit::{consume_or_queue_outbound, RateLimitResult};
use crate::state::{
    address_to_bytes32, extend_persistent_ttl, sequence_to_message_id, use_message_sequence,
    DataKey, OutboundQueuedTransfer, TransferResult,
};
use crate::token_ops::{custody_tokens, get_token_decimals, release_tokens};
use crate::transceivers::get_enabled_transceivers;

/// Sends a transfer message to all enabled transceivers.
///
/// Creates an NTT message, computes its digest, and dispatches it to each
/// enabled transceiver for cross-chain delivery. Uses `use_message_sequence`
/// to assign a unique sequence number. Returns the sequence and digest for
/// tracking and verification.
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
    let sequence = use_message_sequence(env);
    let message_id = sequence_to_message_id(env, sequence);
    let sender_bytes = address_to_bytes32(env, sender);

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

    let our_chain_id: u32 = env
        .storage()
        .instance()
        .get(&DataKey::ChainId)
        .ok_or(NttManagerError::NotInitialized)?;

    let digest = ntt_message.compute_digest(env, our_chain_id as u16);
    let payload = ntt_message.to_bytes(env);

    let transceivers = get_enabled_transceivers(env);
    for transceiver in transceivers.iter() {
        env.invoke_contract::<()>(
            &transceiver,
            &Symbol::new(env, "send_message"),
            vec![
                env,
                recipient_chain.into_val(env),
                recipient_ntt_manager.into_val(env),
                payload.clone().into_val(env),
            ],
        );
    }

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

    let token: Address = env
        .storage()
        .instance()
        .get(&DataKey::Token)
        .ok_or(NttManagerError::NotInitialized)?;
    let our_decimals = get_token_decimals(&env)?;

    let mut transfer_amount = amount as u128;
    let trimmed = TrimmedAmount::remove_dust(
        &mut transfer_amount,
        our_decimals as u8,
        peer.token_decimals as u8,
    );

    custody_tokens(env, sender, transfer_amount as i128)?;

    let rate_result = consume_or_queue_outbound(env, trimmed.amount);

    let source_token = address_to_bytes32(env, &token);

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

            Ok(TransferResult {
                sequence,
                queued: false,
                digest,
            })
        }
        RateLimitResult::Delayed(release_timestamp) => {
            if !should_queue {
                release_tokens(env, sender, transfer_amount as i128)?;
                return Err(NttManagerError::TransferExceedsRateLimit);
            }

            let sequence = use_message_sequence(env);

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

            env.storage()
                .persistent()
                .set(&DataKey::OutboundQueue(sequence), &queued);
            extend_persistent_ttl(env, &DataKey::OutboundQueue(sequence));

            let message_id = sequence_to_message_id(env, sequence);
            let sender_bytes = address_to_bytes32(env, sender);
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
            let our_chain_id: u32 = env
                .storage()
                .instance()
                .get(&DataKey::ChainId)
                .ok_or(NttManagerError::NotInitialized)?;
            let digest = ntt_message.compute_digest(env, our_chain_id as u16);

            Ok(TransferResult {
                sequence,
                queued: true,
                digest,
            })
        }
    }
}

/// Completes a queued outbound transfer after its release timestamp.
///
/// Anyone can call this once `release_timestamp` is reached. Removes the transfer
/// from the queue, attempts to consume rate limit capacity, and sends the message
/// to transceivers. If still rate limited, returns `TransferExceedsRateLimit`.
///
/// The returned sequence number is a *new* sequence (not the original queue sequence).
///
/// # Errors
/// - `TransferNotQueued` if no transfer exists for the given sequence
/// - `TransferNotReleasable` if current time is before release timestamp
/// - `TransferExceedsRateLimit` if rate limit is still exceeded
pub fn complete_outbound_queued_transfer(
    env: &Env,
    sequence: u64,
) -> Result<TransferResult, NttManagerError> {
    let key = DataKey::OutboundQueue(sequence);
    let queued: OutboundQueuedTransfer = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(NttManagerError::TransferNotQueued)?;

    let now = env.ledger().timestamp();
    if now < queued.release_timestamp {
        return Err(NttManagerError::TransferNotReleasable);
    }

    env.storage().persistent().remove(&key);

    let rate_result = consume_or_queue_outbound(env, queued.amount.amount);
    if !matches!(rate_result, RateLimitResult::Consumed) {
        return Err(NttManagerError::TransferExceedsRateLimit);
    }

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

    Ok(TransferResult {
        sequence: new_sequence,
        queued: false,
        digest,
    })
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
    let key = DataKey::OutboundQueue(sequence);
    let queued: OutboundQueuedTransfer = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(NttManagerError::TransferNotQueued)?;

    if queued.sender != *sender {
        return Err(NttManagerError::CancellerNotSender);
    }

    env.storage().persistent().remove(&key);

    let token_decimals = get_token_decimals(env)?;
    let refund_amount = queued.amount.untrim(token_decimals as u8) as i128;

    release_tokens(env, sender, refund_amount)?;

    Ok(())
}
