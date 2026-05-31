//! Queued outbound transfer can be cancelled by the sender, fully refunding the queued amount.

use integration_tests::cli::invoke;
use integration_tests::deploy::{parse_i128, Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const SUPPLY: i128 = 1_000;
const OUTBOUND_LIMIT: u64 = 100;
const QUEUED_AMOUNT: i128 = 150;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            mode: Mode::Burning,
            token_decimals: 8,
            outbound_limit: OUTBOUND_LIMIT,
            rate_limit_duration: 300,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: cancel path orphaning the sender's tokens in the manager —
/// would mean queued transfers can never be reclaimed, only released after
/// the window expires.
#[test]
#[ignore]
fn cancel_queued_outbound_refunds_sender_in_full() {
    let f = setup();

    let recipient = [0xbb; 32];
    let queued = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &QUEUED_AMOUNT.to_string(),
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            "true",
        ],
    );
    let sequence = parse_i128(&queued["sequence"]) as u64;
    assert!(
        queued["queued"].as_bool().unwrap_or(false),
        "transfer over capacity should report queued=true"
    );

    let after_queue = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(after_queue, SUPPLY - QUEUED_AMOUNT);

    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "cancel_queued_transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--sequence",
            &sequence.to_string(),
        ],
    );

    let after_cancel = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(
        after_cancel, SUPPLY,
        "cancel must refund the full queued amount"
    );

    let queue_item = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_outbound_queue_item",
        &["--sequence", &sequence.to_string()],
    );
    assert!(queue_item.is_null(), "cancelled entry must be removed");
}
