//! Localnet integration-test harness for the Stellar NTT contracts.
//!
//! Wraps the `stellar` CLI in typed Rust primitives so tests can deploy the
//! manager / transceiver / fungible-token / Wormhole core stack on a docker
//! standalone network, exercise it under signed auth, decode the events it
//! emits, and assert against typed contract-error codes.
//!
//! Tests live under `tests/<area>/<scenario>.rs` and consume the re-exported
//! [`TestContext`], the per-contract deployers + orchestration in [`deploy`],
//! the event decoder in [`events`], and the message + VAA builders in
//! [`messages`] / [`vaa`]. Every test is `#[ignore]`-gated so
//! `cargo test --workspace` skips them on hosts without docker; the opt-in
//! runner is `scripts/run-tests.sh`.

pub mod cli;
pub mod ctx;
pub mod deploy;
pub mod events;
pub mod messages;
pub mod vaa;

pub use ctx::TestContext;
