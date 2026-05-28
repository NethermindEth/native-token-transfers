use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env};

use crate::errors::TransceiverError;

/// Transport-agnostic interface every transceiver contract must implement.
///
/// A transceiver ferries NTT messages between the local manager and a
/// remote chain. This trait defines the minimal surface the manager
/// relies on: identifying its owning manager and dispatching outbound
/// messages. Specific transports (e.g. Wormhole) extend this with
/// their own verification and peer-management interface.
///
/// The `#[contractclient]` attribute generates a `TransceiverClient`
/// binding the manager uses to invoke transceivers it has registered.
#[contractclient(name = "TransceiverClient")]
pub trait TransceiverInterface {
    /// Returns the address of the manager contract that owns this transceiver.
    fn get_manager(env: Env) -> Result<Address, TransceiverError>;
    /// Returns the 32-byte identifier of the owning manager.
    ///
    /// This is the canonical on-chain representation of the manager used
    /// inside NTT messages, and must match the manager's address payload.
    fn get_manager_id(env: Env) -> Result<BytesN<32>, TransceiverError>;
    /// Returns the token address the owning manager is configured with.
    fn get_manager_token(env: Env) -> Result<Address, TransceiverError>;
    /// Returns the current contract version number.
    fn get_version(env: Env) -> u32;
    /// Returns the transport identifier for this transceiver implementation.
    fn get_transceiver_type(env: Env) -> Bytes;
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
    ) -> Result<(), TransceiverError>;
    /// Returns the cost in stroops of delivering a message to `recipient_chain`.
    ///
    /// Equals the Wormhole core message fee — the Stellar transceiver posts
    /// directly to Wormhole core with no relayer leg, so callers only cover
    /// the `post_message` fee.
    fn quote_delivery_price(
        env: Env,
        recipient_chain: u32,
    ) -> Result<i128, TransceiverError>;
    /// Replaces the contract WASM with the one identified by `new_wasm_hash`.
    ///
    /// The WASM must be installed on the network beforehand. Restricted to the
    /// contract owner (see OZ `Ownable`).
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), TransceiverError>;
}
