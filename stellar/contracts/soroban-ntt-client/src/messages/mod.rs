//! NTT wire-format types.
//!
//! Byte layouts in this module are observed by other chains, off-chain
//! relayers, and indexers. Changing a layout is a protocol-level change.

pub mod trimmed_amount;

pub use trimmed_amount::TrimmedAmount;
