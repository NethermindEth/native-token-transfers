//! The transceiver's own pause gate blocks both outbound and inbound, independently of the manager's pause.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::build_inbound_vaa_hex;
use integration_tests::TestContext;

use crate::common::{
    peer_inbound_vaa, DUMMY_RECIPIENT, PEER_ADDR, PEER_CHAIN, STANDARD_TRIMMED_AMOUNT,
};

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, 1_000_000);
    Fixture { ctx, stack }
}

/// Catches: the transceiver's own pause gate being silently bypassed. Pausing
/// the transceiver alone (the manager stays unpaused) must block both
/// directions — outbound, because the manager's send_message call into the
/// paused transceiver fails and surfaces as the manager's TransceiverCallFailed
/// (#49) wrapper, and inbound, because receive_message is itself pause-gated
/// (#1000 EnforcedPause). A regression here would let transfers flow during a
/// transceiver-level incident pause.
#[test]
#[ignore]
fn transceiver_pause_blocks_outbound_and_inbound() {
    let f = setup();

    f.stack
        .transceiver_pause(&f.ctx.admin_identity, &f.ctx.admin_address);
    assert!(f.stack.transceiver_paused(&f.ctx), "transceiver must be paused");
    assert!(
        !f.stack.paused(&f.ctx),
        "manager must stay unpaused — the two pause gates are independent"
    );

    let out_err = f
        .stack
        .try_transfer(&f.ctx, 1000, PEER_CHAIN, &DUMMY_RECIPIENT, false)
        .expect_err("outbound must fail while the transceiver is paused");
    assert_eq!(
        out_err.code,
        Some(49),
        "expected TransceiverCallFailed (#49, manager wrapper around the transceiver's EnforcedPause), got {:?}",
        out_err.code
    );

    let vaa = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        DUMMY_RECIPIENT,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));
    let in_err = f
        .stack
        .try_receive_message(&f.ctx, &f.stack.transceiver, &vaa)
        .expect_err("inbound must fail while the transceiver is paused");
    assert_eq!(
        in_err.code,
        Some(1000),
        "expected EnforcedPause (#1000) on receive_message, got {:?}",
        in_err.code
    );
}
