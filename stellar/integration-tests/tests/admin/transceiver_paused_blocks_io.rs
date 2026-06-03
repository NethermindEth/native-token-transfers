//! The transceiver's own pause gate blocks both outbound and inbound, independently of the manager's pause.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, stellar_addr_to_bytes32, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::TestContext;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const RECIPIENT: [u8; 32] = [0x40; 32];

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
        .try_transfer(&f.ctx, 1000, PEER_CHAIN, &RECIPIENT, false)
        .expect_err("outbound must fail while the transceiver is paused");
    assert_eq!(
        out_err.code,
        Some(49),
        "expected TransceiverCallFailed (#49, manager wrapper around the transceiver's EnforcedPause), got {:?}",
        out_err.code
    );

    let vaa = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0x10; 32],
            sender: [0x20; 32],
            source_token: [0x30; 32],
            recipient: RECIPIENT,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: 100_000_000,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: stellar_addr_to_bytes32(&f.stack.manager),
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence: 0,
        guardian_secret: &f.ctx.guardian_secret,
    });
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
