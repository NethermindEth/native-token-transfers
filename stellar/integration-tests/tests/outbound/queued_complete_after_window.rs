//! Queued outbound transfer completes after its rate-limit window elapses.

use integration_tests::deploy::{parse_i128, Stack, StackOptions};
use integration_tests::TestContext;

use crate::common::{complete_after_window, DUMMY_RECIPIENT, EVENT_TIMEOUT, PEER_ADDR, PEER_CHAIN};

const SUPPLY: i128 = 1_000;
const OUTBOUND_LIMIT: u64 = 100;
const RATE_LIMIT_DURATION: u64 = 60;
const PRIMER_AMOUNT: i128 = 50;
const QUEUED_AMOUNT: i128 = 75;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            token_decimals: 8,
            outbound_limit: OUTBOUND_LIMIT,
            rate_limit_duration: RATE_LIMIT_DURATION,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: queued outbound transfers that never release after their window —
/// would freeze user funds indefinitely. Also: window expiry being miscomputed
/// against live ledger time vs the test-only mock_ledger time.
#[test]
#[ignore]
fn queued_outbound_releases_after_window() {
    let f = setup();

    f.stack.transfer(&f.ctx, PRIMER_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false);

    let queued = f
        .stack
        .transfer(&f.ctx, QUEUED_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, true);
    let sequence = parse_i128(&queued["sequence"]) as u64;
    assert!(
        queued["queued"].as_bool().unwrap_or(false),
        "transfer over remaining capacity must report queued=true: got {queued}"
    );

    let after_queue = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(
        after_queue,
        SUPPLY - PRIMER_AMOUNT - QUEUED_AMOUNT,
        "queued transfer must debit sender at queue time"
    );

    complete_after_window(|| f.stack.try_complete_queued_transfer(&f.ctx, sequence));

    let queue_item = f.stack.outbound_queue_item(&f.ctx, sequence);
    assert!(
        queue_item.is_null(),
        "queue entry must be removed after release"
    );

    // The primer transfer also emits `transfer_sent`, so match on the queued
    // amount to confirm the *released* transfer actually went out, not just that
    // the queue entry vanished.
    let released = f.stack.manager_events(&f.ctx).find(EVENT_TIMEOUT, |ev| {
        ev.topic_symbol(0).as_deref() == Some("transfer_sent")
            && ev.data_u64("amount") == Some(QUEUED_AMOUNT as u64)
    });
    assert!(
        released.is_some(),
        "releasing the queued transfer must emit transfer_sent for the queued amount"
    );
}
