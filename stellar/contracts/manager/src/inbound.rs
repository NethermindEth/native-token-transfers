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
use crate::transceivers::{get_enabled_bitmap, get_threshold, get_transceiver, get_transceiver_index};

pub fn attestation_received_internal(
    env: &Env,
    transceiver: &Address,
    source_chain: u32,
    source_ntt_manager: &BytesN<32>,
    payload: &Bytes,
) -> Result<AttestationResult, NttManagerError> {
    let transceiver_index = get_transceiver_index(env, transceiver)
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    let transceiver_info = get_transceiver(env, transceiver_index)
        .ok_or(NttManagerError::TransceiverNotRegistered)?;

    if !transceiver_info.enabled {
        return Err(NttManagerError::TransceiverNotEnabled);
    }

    verify_peer(env, source_chain, source_ntt_manager)?;

    let ntt_message = NttManagerMessage::from_bytes(env, payload)?;

    let digest = ntt_message.compute_digest(env, source_chain as u16);

    let key = DataKey::Attestation(digest.clone());
    let mut attestation: AttestationInfo = env
        .storage()
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
