//! End-to-end inbound locking happy path: signed VAA releases pre-deposited SAC custody to recipient.

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_bytes32};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

use crate::common::{
    peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS, STANDARD_RECEIVED,
    STANDARD_TRIMMED_AMOUNT,
};

const PREFUND_XLM: i128 = 100_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_addr: String,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            mode: Mode::Locking,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);

    let recipient_addr = ctx.setup_identity("recipient_unlock");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    invoke(
        &ctx.admin_identity,
        &stack.token,
        "transfer",
        &[
            "--from",
            &ctx.admin_address,
            "--to",
            &stack.manager,
            "--amount",
            &PREFUND_XLM.to_string(),
        ],
    );

    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: locking-mode inbound break where the manager fails to release SAC
/// custody to the recipient (would freeze cross-chain inbound on the canonical
/// chain), or releases the wrong amount due to misapplied untrim math.
#[test]
#[ignore]
fn inbound_locking_unlocks_recipient() {
    let f = setup();
    let manager_before = f.stack.token_balance(&f.ctx, &f.stack.manager);
    let recipient_before = f.stack.token_balance(&f.ctx, &f.recipient_addr);

    let vaa_hex = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    let manager_after = f.stack.token_balance(&f.ctx, &f.stack.manager);
    let recipient_after = f.stack.token_balance(&f.ctx, &f.recipient_addr);

    assert_eq!(
        manager_before - manager_after,
        STANDARD_RECEIVED,
        "manager XLM must drop by the unlocked amount"
    );
    assert_eq!(
        recipient_after - recipient_before,
        STANDARD_RECEIVED,
        "recipient XLM must rise by the unlocked amount"
    );
}
