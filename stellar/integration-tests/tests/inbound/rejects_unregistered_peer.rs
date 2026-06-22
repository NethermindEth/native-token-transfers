//! Manager-side peer check fires on inbound from a chain whose peer was never registered (surfaces as #36 through the transceiver wrapper).

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_hash};
use integration_tests::TestContext;

use crate::common::{peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, STANDARD_TRIMMED_AMOUNT};

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_transceiver_peer_only(&ctx, PEER_CHAIN, &PEER_ADDR);
    let recipient_addr = ctx.setup_identity("recipient_np");
    let recipient_bytes32 = stellar_addr_to_hash(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_bytes32,
    }
}

/// Catches: the manager not rejecting an inbound transfer from a peer chain
/// that has no registered manager peer — would let arbitrary cross-chain
/// attackers credit our recipients. The transceiver wraps the manager error
/// as ManagerRejectedMessage (#36), so we assert #36; the fact that the call
/// errors at all proves the manager-side check fired.
#[test]
#[ignore]
fn inbound_from_unregistered_manager_peer_errors_36() {
    let f = setup();

    let vaa_hex = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));

    let err = f
        .stack
        .try_receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex)
        .expect_err("receive_message must reject when manager has no peer for source chain");
    assert_eq!(
        err.code,
        Some(36),
        "expected ManagerRejectedMessage (#36, transceiver wrapper around manager's #50 PeerNotFound), got {:?}",
        err.code
    );
}
