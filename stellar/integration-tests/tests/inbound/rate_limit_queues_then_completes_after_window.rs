use std::{thread, time::Duration};

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, compute_message_digest, InboundVaaInputs,
    NttManagerMessageInputs,
};
use integration_tests::vaa::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const PEER_DECIMALS: u32 = 8;
const INBOUND_LIMIT: u64 = 150_000_000;
const RATE_LIMIT_DURATION: u64 = 60;
const PRIMER_TRIMMED: u64 = 100_000_000;
const QUEUED_TRIMMED: u64 = 75_000_000;
const PRIMER_MINT: i128 = 10_000_000;
const QUEUED_MINT: i128 = 7_500_000;
const WAIT_SECONDS: u64 = 18;

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
            mode: Mode::Burning,
            token_decimals: 7,
            outbound_limit: u64::MAX,
            rate_limit_duration: RATE_LIMIT_DURATION,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, INBOUND_LIMIT);
    let recipient_addr = ctx.setup_identity("recipient_rl");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

fn send_vaa(
    f: &Fixture,
    id: [u8; 32],
    trimmed: u64,
    sequence: u64,
) -> NttManagerMessageInputs {
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    let ntt = NttManagerMessageInputs {
        id,
        sender: [0x22; 32],
        source_token: [0x33; 32],
        recipient: f.recipient_bytes32,
        recipient_chain: f.ctx.stellar_chain_id,
        trimmed_amount: trimmed,
        trimmed_decimals: 8,
    };
    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt,
        source_manager: PEER_ADDR,
        recipient_manager: manager_bytes32,
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence,
        guardian_secret: &f.ctx.guardian_secret,
    });
    invoke(
        &f.ctx.admin_identity,
        &f.stack.transceiver,
        "receive_message",
        &["--vaa_bytes", &vaa_hex],
    );
    ntt
}

/// Catches: queued inbound transfers that never release after their window —
/// would freeze recipient funds indefinitely on the canonical chain. Also:
/// window expiry being miscomputed against live ledger time.
#[test]
#[ignore]
fn queued_inbound_releases_after_window() {
    let f = setup();

    send_vaa(&f, [0x40; 32], PRIMER_TRIMMED, 0);
    let after_primer = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(after_primer, PRIMER_MINT);

    let queued_ntt = send_vaa(&f, [0x41; 32], QUEUED_TRIMMED, 1);
    let after_queue = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_queue, PRIMER_MINT,
        "queued VAA must not mint before window expires"
    );

    let digest = compute_message_digest(&queued_ntt, PEER_CHAIN as u16);
    let digest_hex = hex::encode(digest);

    let queue_item = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_inbound_queue_item",
        &["--digest", &digest_hex],
    );
    assert!(
        !queue_item.is_null(),
        "second VAA must have created a queue entry: got {queue_item}"
    );

    thread::sleep(Duration::from_secs(WAIT_SECONDS));

    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "complete_inbound_transfer",
        &["--digest", &digest_hex],
    );

    let final_balance = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        final_balance,
        PRIMER_MINT + QUEUED_MINT,
        "after window release, recipient must have both mints"
    );

    let queue_after = invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "get_inbound_queue_item",
        &["--digest", &digest_hex],
    );
    assert!(queue_after.is_null(), "queue entry must be removed");
}
