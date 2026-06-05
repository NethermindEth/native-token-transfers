//! Single test-binary driver. Each `mod` declaration pulls in one area
//! folder; each area's `mod.rs` lists its scenario submodules. Every
//! `#[test] #[ignore]` function under these modules is discovered by Cargo
//! and run by `scripts/run-tests.sh`.

mod admin;
mod common;
mod cross_cutting;
mod inbound;
mod outbound;
