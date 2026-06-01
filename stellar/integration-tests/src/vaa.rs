//! Host-side guardian signing for inbound VAAs, shared with the in-process unit
//! suite via the [`vaa_test_signing`] crate. Re-exported as `crate::vaa` so
//! `deploy.rs` and `messages.rs` reference it unchanged.

pub use vaa_test_signing::{assemble, eth_address_from_privkey, keccak256, sign, GuardianSignature};
