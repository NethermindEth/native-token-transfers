//! Disabling a transceiver auto-reduces the threshold so the surviving transceiver can still execute alone.

use integration_tests::cli::invoke;
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
    stack.set_threshold(&ctx, 2);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_disable");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: the manager not auto-reducing the threshold when a transceiver is
/// disabled — which would either leave threshold=2 with only one enabled
/// transceiver (every inbound transfer permanently stuck below quorum) or
/// violate the threshold ≤ enabled-count invariant. After disabling the second
/// transceiver, the threshold must drop to 1 and a single attestation from the
/// surviving transceiver must execute.
#[test]
#[ignore]
fn disabling_transceiver_drops_threshold_and_executes_single_attestation() {
    let f = setup();

    f.stack.remove_transceiver(&f.ctx, &f.transceiver_two);

    let threshold = invoke(&f.ctx.admin_identity, &f.stack.manager, "get_threshold", &[]);
    assert_eq!(
        threshold.as_u64(),
        Some(1),
        "disabling one of two transceivers under threshold=2 must auto-drop threshold to 1"
    );

    let vaa = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));
    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa);

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        STANDARD_RECEIVED,
        "after the threshold auto-drop, a single attestation must execute"
    );
}
