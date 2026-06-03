//! Asserts that raising the attestation threshold enforces N-of-M quorum on inbound execution.

use std::time::Duration;

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_bytes32};
use integration_tests::TestContext;

use crate::common::{
    peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, STANDARD_RECEIVED, STANDARD_TRIMMED_AMOUNT,
};

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    transceiver_two: String,
    recipient_addr: String,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    let transceiver_two = stack.deploy_extra_transceiver(&ctx);
    stack.register_transceiver_addr(&ctx, &transceiver_two);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_thresh");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

fn build_vaa(f: &Fixture, sequence: u64) -> String {
    build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        sequence,
    ))
}

/// Catches: a regression in threshold enforcement — either a single
/// attestation executing despite threshold=2 (no quorum at all), or a
/// quorum-met second attestation failing to execute (live-locked).
/// Also catches: `set_threshold` not emitting the `threshold_changed`
/// event, which off-chain monitors rely on.
#[test]
#[ignore]
fn raise_threshold_requires_quorum_before_execution() {
    let f = setup();

    f.stack.set_threshold(&f.ctx, 2);
    let event = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("threshold_changed", Duration::from_secs(10));
    assert!(event.is_some(), "set_threshold(2) must emit threshold_changed");

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &build_vaa(&f, 0));
    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        0,
        "single attestation must not execute under threshold=2"
    );

    f.stack.receive_message(&f.ctx, &f.transceiver_two, &build_vaa(&f, 1));
    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        STANDARD_RECEIVED,
        "quorum met, recipient must be minted"
    );
}
