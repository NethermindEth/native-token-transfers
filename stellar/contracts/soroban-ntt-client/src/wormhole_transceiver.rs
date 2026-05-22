use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env};

use crate::errors::TransceiverError;
use crate::types::PeerInfo;

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
    fn get_wormhole_core(env: Env) -> Result<Address, TransceiverError>;
    /// Registers a new peer transceiver for `chain_id`.
    ///
    /// Rejects zero chain IDs, chain IDs above `u16::MAX`, zero emitter
    /// addresses, and chains that already have a peer registered.
    /// The peer is enabled by default.
    fn set_peer(
        env: Env,
        chain_id: u32,
        emitter: BytesN<32>,
    ) -> Result<(), TransceiverError>;
    /// Updates the emitter address of an existing peer without changing its
    /// enabled state. Fails if no peer is registered for `chain_id`.
    fn update_peer(
        env: Env,
        chain_id: u32,
        emitter: BytesN<32>,
    ) -> Result<(), TransceiverError>;
    /// Enables or disables message flow with an existing peer.
    ///
    /// A disabled peer still has its address preserved, so re-enabling
    /// does not require re-authorizing the emitter.
    fn set_peer_enabled(
        env: Env,
        chain_id: u32,
        enabled: bool,
    ) -> Result<(), TransceiverError>;
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
    fn receive_message(env: Env, vaa_bytes: Bytes) -> Result<(), TransceiverError>;
    /// Returns whether this VAA has already been consumed.
    fn is_vaa_consumed(
        env: Env,
        emitter_chain: u32,
        emitter_address: BytesN<32>,
        sequence: u64,
    ) -> bool;
}
