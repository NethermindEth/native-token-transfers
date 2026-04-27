use soroban_sdk::{contractclient, contracterror, contracttype, Address, Bytes, BytesN, Env};

/// Registration entry for a peer Wormhole transceiver on another chain.
///
/// Stored per-chain and consulted on both outbound dispatch (to locate
/// the recipient transceiver) and inbound verification (to reject VAAs
/// from unexpected emitters).
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct PeerInfo {
    /// 32-byte Wormhole emitter address of the peer transceiver.
    pub emitter: BytesN<32>,
    /// Whether this peer is currently accepting messages in either direction.
    pub enabled: bool,
}

/// Errors returned by the Wormhole Transceiver contract.
///
/// Variants are grouped by numeric range:
/// - `1..=6`: initialization and core wiring
/// - `10..=15`: peer registration and chain ID validation
/// - `20..=22`: Wormhole core interactions (verify, parse, post)
/// - `30..=36`: NTT message decoding and attestation dispatch
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[contracterror]
#[repr(u32)]
pub enum TransceiverError {
    /// Transceiver has not been initialized.
    NotInitialized = 1,
    /// Transceiver has already been initialized and cannot be re-initialized.
    AlreadyInitialized = 2,
    /// Caller is not authorized for the requested operation.
    Unauthorized = 3,
    /// Provided manager ID does not match the hash derived from the manager address.
    InvalidManagerId = 4,
    /// Manager address has not been set in instance storage.
    ManagerNotSet = 5,
    /// Wormhole core contract address has not been set.
    WormholeCoreNotSet = 6,
    /// Peer chain ID cannot be zero.
    InvalidPeerChainIdZero = 10,
    /// Peer emitter address is the zero address.
    InvalidPeerZeroAddress = 11,
    /// A peer is already registered for the given chain ID; use `update_peer` instead.
    PeerAlreadySet = 12,
    /// No peer is registered for the given chain ID.
    PeerNotFound = 13,
    /// The peer for this chain is registered but currently disabled.
    PeerDisabled = 14,
    /// Chain ID exceeds `u16::MAX` (the Wormhole chain ID range).
    ChainIdTooLarge = 15,
    /// Wormhole core rejected the VAA signature or guardian set verification.
    WormholeVerificationFailed = 20,
    /// Wormhole core failed to parse the VAA bytes.
    WormholeParseFailed = 21,
    /// Wormhole core rejected the outbound `post_message` call.
    WormholePostFailed = 22,
    /// Decoded NTT message prefix did not match the Wormhole transceiver magic bytes.
    InvalidTransceiverPrefix = 30,
    /// NTT message payload is shorter than the minimum transceiver message length.
    MessageTooShort = 31,
    /// Outbound manager payload exceeds the maximum encodable length (`u16::MAX`).
    PayloadTooLong = 32,
    /// Recipient manager ID in the decoded message does not match this transceiver's manager.
    UnexpectedRecipientManager = 33,
    /// A VAA with the same `(emitter_chain, emitter_address, sequence)` has already been consumed.
    ReplayDetected = 34,
    /// VAA emitter address does not match the registered peer emitter for its chain.
    UnexpectedEmitter = 35,
    /// The downstream manager rejected the forwarded attestation.
    ManagerRejectedMessage = 36,
}

/// Wormhole-specific management and inbound message handling for a transceiver.
///
/// Complements [`TransceiverInterface`](crate::transceiver::TransceiverInterface)
/// (which covers the manager-facing outbound path) with:
/// - per-chain peer registration for Wormhole emitters
/// - inbound VAA verification, replay protection, and forwarding to the manager
///
/// The `#[contractclient]` attribute generates a `WormholeTransceiverClient`
/// binding for callers that need to drive these operations.
#[contractclient(name = "WormholeTransceiverClient")]
pub trait WormholeTransceiverInterface {
    /// Returns the address of the Wormhole core contract this transceiver uses.
    fn get_wormhole_core(env: Env) -> Address;
    /// Registers a new peer transceiver for `chain_id`.
    ///
    /// Rejects zero chain IDs, chain IDs above `u16::MAX`, zero emitter
    /// addresses, and chains that already have a peer registered.
    /// The peer is enabled by default.
    fn set_peer(env: Env, chain_id: u32, emitter: BytesN<32>);
    /// Updates the emitter address of an existing peer without changing its
    /// enabled state. Fails if no peer is registered for `chain_id`.
    fn update_peer(env: Env, chain_id: u32, emitter: BytesN<32>);
    /// Enables or disables message flow with an existing peer.
    ///
    /// A disabled peer still has its address preserved, so re-enabling
    /// does not require re-authorizing the emitter.
    fn set_peer_enabled(env: Env, chain_id: u32, enabled: bool);
    /// Returns the emitter address of the peer registered for `chain_id`,
    /// or `None` if no peer exists (regardless of enabled state).
    fn get_peer(env: Env, chain_id: u32) -> Option<BytesN<32>>;
    /// Returns the full [`PeerInfo`] (emitter + enabled flag) for `chain_id`.
    fn get_peer_info(env: Env, chain_id: u32) -> Option<PeerInfo>;
    /// Returns `true` when a peer is registered for `chain_id` and enabled.
    fn is_peer_enabled(env: Env, chain_id: u32) -> bool;
    /// Verifies and processes an inbound Wormhole VAA.
    ///
    /// The VAA is verified by the Wormhole core contract, parsed, matched
    /// against the registered peer, protected against replay via the
    /// `(emitter_chain, emitter_address, sequence)` tuple, and then the
    /// decoded manager payload is forwarded to the owning manager via
    /// `attestation_received`.
    fn receive_message(env: Env, vaa_bytes: Bytes);
    /// Returns whether this VAA has already been consumed.
    fn is_vaa_consumed(
        env: Env,
        emitter_chain: u32,
        emitter_address: BytesN<32>,
        sequence: u64,
    ) -> bool;
    /// Deprecated alias for [`receive_message`](Self::receive_message),
    /// kept for compatibility with callers that used the older name.
    fn receive_vaa(env: Env, vaa_bytes: Bytes);
}
