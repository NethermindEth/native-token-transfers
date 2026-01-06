//! Outbound transfer operations and queue management.
//!
//! This module handles the complete lifecycle of outbound cross-chain transfers:
//! - Immediate transfers that pass rate limiting
//! - Queued transfers that exceed rate limits
//! - Queue completion and cancellation (implemented in tasks 7.4)

use soroban_sdk::{vec, Address, Bytes, BytesN, Env, IntoVal, Symbol};

use crate::constants::{PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD};
use crate::errors::NttManagerError;
use crate::messages::{NativeTokenTransfer, NttManagerMessage, TrimmedAmount};
use crate::peers::{get_peer, refill_inbound};
use crate::rate_limit::{consume_or_queue_outbound, RateLimitResult};
use crate::state::{
    address_to_bytes32, sequence_to_message_id, use_message_sequence, DataKey,
    OutboundQueuedTransfer, TransferResult,
};
use crate::token_ops::{custody_tokens, get_token_decimals, release_tokens};
use crate::transceivers::get_enabled_transceivers;

/// Extends the TTL for a specific persistent storage key.
///
/// Used for per-chain/per-message data like peers, attestations, and queues.
fn extend_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
}

/// Sends a transfer message to all enabled transceivers.
///
/// Creates an NTT message with the given parameters, computes its digest,
/// and dispatches it to each enabled transceiver for cross-chain delivery.
///
/// Returns the message sequence number and digest.
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

/// Internal transfer implementation with rate limiting and queueing.
///
/// Validates the transfer, takes custody of tokens, and either:
/// - Sends immediately if rate limit allows, or
/// - Queues for later completion if rate limited and `should_queue` is true
///
/// Returns a `TransferResult` indicating the sequence, queue status, and digest.
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
