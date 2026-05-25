//! Protocol events emitted by the NTT manager and transceiver contracts.
//!
//! Event structs live in this crate so cross-contract observers — off-chain
//! indexers, the NTT Accountant, integration tests — consume them via the
//! published definition. Each struct has a companion `emit_*` helper that
//! contracts call after a successful state transition.

use soroban_sdk::{contractevent, Address, Bytes, BytesN, Env};

// Manager events.

/// Outbound transfer initiated and forwarded to enabled transceivers.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferSent {
    #[topic]
    pub recipient_chain: u32,
    #[topic]
    pub digest: BytesN<32>,
    pub recipient: BytesN<32>,
    pub amount: u64,
    pub fee: i128,
    pub msg_sequence: u64,
}

pub fn emit_transfer_sent(
    env: &Env,
    recipient: &BytesN<32>,
    amount: u64,
    fee: i128,
    recipient_chain: u32,
    msg_sequence: u64,
    digest: &BytesN<32>,
) {
    TransferSent {
        recipient_chain,
        digest: digest.clone(),
        recipient: recipient.clone(),
        amount,
        fee,
        msg_sequence,
    }
    .publish(env);
}

/// Inbound message cleared the attestation threshold; tokens released to the recipient.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferRedeemed {
    #[topic]
    pub digest: BytesN<32>,
}

pub fn emit_transfer_redeemed(env: &Env, digest: &BytesN<32>) {
    TransferRedeemed { digest: digest.clone() }.publish(env);
}

/// A registered transceiver attested to an inbound message digest.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAttestedTo {
    #[topic]
    pub digest: BytesN<32>,
    #[topic]
    pub transceiver: Address,
    pub index: u32,
}

pub fn emit_message_attested_to(
    env: &Env,
    digest: &BytesN<32>,
    transceiver: &Address,
    index: u32,
) {
    MessageAttestedTo {
        digest: digest.clone(),
        transceiver: transceiver.clone(),
        index,
    }
    .publish(env);
}

/// Inbound attestation threshold changed.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdChanged {
    pub old_threshold: u32,
    pub new_threshold: u32,
}

pub fn emit_threshold_changed(env: &Env, old_threshold: u32, new_threshold: u32) {
    ThresholdChanged { old_threshold, new_threshold }.publish(env);
}

/// New transceiver registered with the manager.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransceiverAdded {
    #[topic]
    pub transceiver: Address,
    pub transceivers_num: u32,
    pub threshold: u32,
}

pub fn emit_transceiver_added(
    env: &Env,
    transceiver: &Address,
    transceivers_num: u32,
    threshold: u32,
) {
    TransceiverAdded {
        transceiver: transceiver.clone(),
        transceivers_num,
        threshold,
    }
    .publish(env);
}

/// Transceiver removed from the active set.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransceiverRemoved {
    #[topic]
    pub transceiver: Address,
    pub threshold: u32,
}

pub fn emit_transceiver_removed(env: &Env, transceiver: &Address, threshold: u32) {
    TransceiverRemoved { transceiver: transceiver.clone(), threshold }.publish(env);
}

/// Peer manager registration created or overwritten for a remote chain.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerUpdated {
    #[topic]
    pub chain_id: u32,
    pub old_peer_contract: BytesN<32>,
    pub old_peer_decimals: u32,
    pub peer_contract: BytesN<32>,
    pub peer_decimals: u32,
}

pub fn emit_peer_updated(
    env: &Env,
    chain_id: u32,
    old_peer_contract: &BytesN<32>,
    old_peer_decimals: u32,
    peer_contract: &BytesN<32>,
    peer_decimals: u32,
) {
    PeerUpdated {
        chain_id,
        old_peer_contract: old_peer_contract.clone(),
        old_peer_decimals,
        peer_contract: peer_contract.clone(),
        peer_decimals,
    }
    .publish(env);
}

/// Queued outbound transfer cancelled by its original sender.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundTransferCancelled {
    #[topic]
    pub sequence: u64,
    pub recipient: Address,
    pub amount: u64,
}

pub fn emit_outbound_transfer_cancelled(
    env: &Env,
    sequence: u64,
    recipient: &Address,
    amount: u64,
) {
    OutboundTransferCancelled {
        sequence,
        recipient: recipient.clone(),
        amount,
    }
    .publish(env);
}

/// Outbound transfer deferred to the rate-limit queue.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundTransferQueued {
    #[topic]
    pub queue_sequence: u64,
}

pub fn emit_outbound_transfer_queued(env: &Env, queue_sequence: u64) {
    OutboundTransferQueued { queue_sequence }.publish(env);
}

/// Companion to [`OutboundTransferQueued`] carrying the sender context.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundTransferRateLimited {
    #[topic]
    pub sender: Address,
    pub sequence: u64,
    pub amount: u64,
    pub current_capacity: u64,
}

pub fn emit_outbound_transfer_rate_limited(
    env: &Env,
    sender: &Address,
    sequence: u64,
    amount: u64,
    current_capacity: u64,
) {
    OutboundTransferRateLimited {
        sender: sender.clone(),
        sequence,
        amount,
        current_capacity,
    }
    .publish(env);
}

/// Inbound transfer deferred to the rate-limit queue.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundTransferQueued {
    #[topic]
    pub digest: BytesN<32>,
}

pub fn emit_inbound_transfer_queued(env: &Env, digest: &BytesN<32>) {
    InboundTransferQueued { digest: digest.clone() }.publish(env);
}

/// Late attestation arrived for a message whose digest was already executed.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAlreadyExecuted {
    #[topic]
    pub source_ntt_manager: BytesN<32>,
    #[topic]
    pub msg_hash: BytesN<32>,
}

pub fn emit_message_already_executed(
    env: &Env,
    source_ntt_manager: &BytesN<32>,
    msg_hash: &BytesN<32>,
) {
    MessageAlreadyExecuted {
        source_ntt_manager: source_ntt_manager.clone(),
        msg_hash: msg_hash.clone(),
    }
    .publish(env);
}

// Transceiver events.

/// Manager-originated outbound message posted to the Wormhole core contract.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSent {
    #[topic]
    pub recipient_chain: u32,
    #[topic]
    pub recipient_manager: BytesN<32>,
    pub manager_payload: Bytes,
}

pub fn emit_message_sent(
    env: &Env,
    recipient_chain: u32,
    recipient_manager: &BytesN<32>,
    manager_payload: &Bytes,
) {
    MessageSent {
        recipient_chain,
        recipient_manager: recipient_manager.clone(),
        manager_payload: manager_payload.clone(),
    }
    .publish(env);
}

/// Inbound VAA verified by the transceiver and forwarded to the manager.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageReceived {
    #[topic]
    pub digest: BytesN<32>,
    #[topic]
    pub emitter_chain: u32,
    pub emitter_address: BytesN<32>,
    pub sequence: u64,
}

pub fn emit_message_received(
    env: &Env,
    digest: &BytesN<32>,
    emitter_chain: u32,
    emitter_address: &BytesN<32>,
    sequence: u64,
) {
    MessageReceived {
        digest: digest.clone(),
        emitter_chain,
        emitter_address: emitter_address.clone(),
        sequence,
    }
    .publish(env);
}

/// Peer transceiver registered for a remote chain.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerSet {
    #[topic]
    pub chain_id: u32,
    pub peer_contract: BytesN<32>,
}

pub fn emit_peer_set(env: &Env, chain_id: u32, peer_contract: &BytesN<32>) {
    PeerSet { chain_id, peer_contract: peer_contract.clone() }.publish(env);
}
