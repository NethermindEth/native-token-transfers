//! NTT wire-format types.
//!
//! Byte layouts in this module are observed by other chains, off-chain
//! relayers, and indexers. Changing a layout is a protocol-level change.

pub mod native_token_transfer;
pub mod ntt_manager_message;
pub mod trimmed_amount;

pub use native_token_transfer::NativeTokenTransfer;
pub use ntt_manager_message::NttManagerMessage;
pub use trimmed_amount::TrimmedAmount;
