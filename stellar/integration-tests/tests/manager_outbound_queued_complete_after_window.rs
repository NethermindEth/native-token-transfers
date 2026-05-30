use std::{thread, time::Duration};

use integration_tests::cli::invoke;
use integration_tests::deploy::{parse_i128, Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const SUPPLY: i128 = 1_000;
const OUTBOUND_LIMIT: u64 = 100;
const RATE_LIMIT_DURATION: u64 = 60;
const PRIMER_AMOUNT: i128 = 50;
const QUEUED_AMOUNT: i128 = 75;
const WAIT_SECONDS: u64 = 18;

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
            rate_limit_duration: RATE_LIMIT_DURATION,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

fn send_transfer(f: &Fixture, amount: i128, should_queue: bool) -> serde_json::Value {
    let recipient = [0xbb; 32];
    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &amount.to_string(),
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            if should_queue { "true" } else { "false" },
        ],
    )
}

/// Catches: queued outbound transfers that never release after their window —
/// would freeze user funds indefinitely. Also: window expiry being miscomputed
/// against live ledger time vs the test-only mock_ledger time.
#[test]
#[ignore]
fn queued_outbound_releases_after_real_clock_window() {
    let f = setup();

    send_transfer(&f, PRIMER_AMOUNT, false);

    let queued = send_transfer(&f, QUEUED_AMOUNT, true);
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

    thread::sleep(Duration::from_secs(WAIT_SECONDS));

    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "complete_queued_transfer",
        &["--sequence", &sequence.to_string()],
    );

    let queue_item = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_outbound_queue_item",
        &["--sequence", &sequence.to_string()],
    );
    assert!(
        queue_item.is_null(),
        "queue entry must be removed after release"
    );
}
