//! Asserts default deploy state and that registering the first transceiver auto-bumps threshold to 1 and persists peer state across both registries.

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

use crate::common::{PEER_ADDR, PEER_CHAIN, PEER_DECIMALS};

const PEER_INBOUND_LIMIT: u64 = 1_000_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    Fixture { ctx, stack }
}

/// Catches three distinct regressions in one path: (a) wrong default state on
/// fresh deploy (paused or non-zero threshold pre-registration would let an
/// outbound transfer either revert with EnforcedPause or pass the threshold
/// invariant check spuriously); (b) `set_transceiver` failing to auto-bump
/// threshold from 0→1 on first registration (would surface later as
/// `ZeroThreshold` at the next outbound attempt, far from the cause); (c)
/// `set_peer` / `transceiver.set_peer` silently succeeding without persisting
/// peer state.
#[test]
#[ignore]
fn register_transceiver_and_peer_round_trip_via_getters() {
    let f = setup();

    let initial_paused =
        invoke(&f.ctx.admin_identity, &f.stack.manager, "paused", &[]);
    assert_eq!(
        initial_paused.as_bool(),
        Some(false),
        "fresh deploy must not be paused"
    );

    let initial_threshold = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_threshold",
        &[],
    );
    assert_eq!(
        initial_threshold.as_u64(),
        Some(0),
        "threshold must start at 0 before any transceiver is registered"
    );

    f.stack.register_transceiver(&f.ctx);
    let threshold = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_threshold",
        &[],
    );
    assert_eq!(
        threshold.as_u64(),
        Some(1),
        "first transceiver should auto-bump threshold to 1"
    );

    let count = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_transceiver_count",
        &[],
    );
    assert_eq!(count.as_u64(), Some(1));

    f.stack
        .register_peer(&f.ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, PEER_INBOUND_LIMIT);

    let peer = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_peer",
        &["--chain_id", &PEER_CHAIN.to_string()],
    );
    let peer_obj = peer.as_object().expect("peer should round-trip as object");
    assert_eq!(
        peer_obj["token_decimals"].as_u64(),
        Some(u64::from(PEER_DECIMALS))
    );

    let tcv_peer = invoke(
        &f.ctx.admin_identity,
        &f.stack.transceiver,
        "get_peer",
        &["--chain_id", &PEER_CHAIN.to_string()],
    );
    assert_eq!(
        tcv_peer.as_str(),
        Some(hex::encode(PEER_ADDR).as_str()),
        "transceiver peer should match registered emitter"
    );
}
