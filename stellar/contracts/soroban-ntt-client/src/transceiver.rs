use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env};

/// Transport-agnostic interface every transceiver contract must implement.
///
/// A transceiver ferries NTT messages between the local manager and a
/// remote chain. This trait defines the minimal surface the manager
/// relies on: identifying its owning manager and dispatching outbound
/// messages. Specific transports (e.g. Wormhole) extend this with their
/// own interface for admin and inbound handling.
///
/// The `#[contractclient]` attribute generates a `TransceiverClient`
/// binding the manager uses to invoke transceivers it has registered.
#[contractclient(name = "TransceiverClient")]
pub trait TransceiverInterface {
    /// Returns the address of the manager contract that owns this transceiver.
    fn get_manager(env: Env) -> Address;
    /// Returns the 32-byte identifier of the owning manager.
    ///
    /// This is the canonical on-chain representation of the manager used
    /// inside NTT messages, and must match the manager's address payload.
    fn get_manager_id(env: Env) -> BytesN<32>;
    /// Dispatches an outbound NTT message to the peer on `recipient_chain`.
    ///
    /// Invoked by the manager after it has locked or burned tokens. The
    /// transceiver wraps the opaque `manager_payload` in its own transport
    /// envelope (prefix, routing fields) and emits it via its transport
    /// (e.g. by posting a Wormhole message).
    ///
    /// Only the owning manager is authorized to call this.
    fn send_message(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    );
}
