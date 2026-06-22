//! Inbound queue + window release: an over-limit VAA queues, then completes after the wall-clock window elapses.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, compute_message_digest, stellar_addr_to_hash,
    NttManagerMessageInputs,
};
use integration_tests::TestContext;

use crate::common::{complete_after_window, peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS};

const INBOUND_LIMIT: u64 = 150_000_000;
const RATE_LIMIT_DURATION: u64 = 60;
const PRIMER_TRIMMED: u64 = 100_000_000;
const QUEUED_TRIMMED: u64 = 75_000_000;
const PRIMER_MINT: i128 = 10_000_000;
const QUEUED_MINT: i128 = 7_500_000;

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
            rate_limit_duration: RATE_LIMIT_DURATION,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, INBOUND_LIMIT);
    let recipient_addr = ctx.setup_identity("recipient_rl");
    let recipient_bytes32 = stellar_addr_to_hash(&recipient_addr);
    stack.record_address(&ctx, &recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

fn send_vaa(f: &Fixture, trimmed: u64, sequence: u64) -> NttManagerMessageInputs {
    let inputs = peer_inbound_vaa(&f.ctx, &f.stack.manager, f.recipient_bytes32, trimmed, sequence);
    f.stack
        .receive_message(&f.ctx, &f.stack.transceiver, &build_inbound_vaa_hex(&inputs));
    inputs.ntt
}

/// Catches: queued inbound transfers that never release after their window —
/// would freeze recipient funds indefinitely on the canonical chain. Also:
/// window expiry being miscomputed against live ledger time.
#[test]
#[ignore]
fn queued_inbound_releases_after_window() {
    let f = setup();

    send_vaa(&f, PRIMER_TRIMMED, 0);
    let after_primer = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(after_primer, PRIMER_MINT);

    let queued_ntt = send_vaa(&f, QUEUED_TRIMMED, 1);
    let after_queue = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_queue, PRIMER_MINT,
        "queued VAA must not mint before window expires"
    );

    let digest = compute_message_digest(&queued_ntt, PEER_CHAIN as u16);
    let digest_hex = hex::encode(digest);

    let queue_item = f.stack.inbound_queue_item(&f.ctx, &digest_hex);
    assert!(
        !queue_item.is_null(),
        "second VAA must have created a queue entry: got {queue_item}"
    );

    complete_after_window(|| f.stack.try_complete_inbound_transfer(&f.ctx, &digest_hex));

    let final_balance = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        final_balance,
        PRIMER_MINT + QUEUED_MINT,
        "after window release, recipient must have both mints"
    );

    let queue_after = f.stack.inbound_queue_item(&f.ctx, &digest_hex);
    assert!(queue_after.is_null(), "queue entry must be removed");
}
