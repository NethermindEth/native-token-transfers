//! End-to-end outbound burning happy path: sender burns, manager balance stays zero, `transfer_sent` fires.

use std::time::Duration;

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const SUPPLY: i128 = 100_000_000;
const TRANSFER_AMOUNT: i128 = 60_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: burning mode wrongly crediting the manager contract — would
/// double-supply tokens across the bridge (peer chain mints AND manager
/// holds escrow simultaneously).
#[test]
#[ignore]
fn outbound_burning_burns_sender_keeps_manager_at_zero() {
    let f = setup();

    let manager_before = f.stack.token_balance(&f.ctx, &f.stack.manager);
    assert_eq!(manager_before, 0, "manager must hold no tokens in burning mode");

    let recipient = [0xbb; 32];
    f.stack.transfer(&f.ctx, TRANSFER_AMOUNT, PEER_CHAIN, &recipient, false);

    let sender_after = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    let manager_after = f.stack.token_balance(&f.ctx, &f.stack.manager);

    assert_eq!(
        sender_after,
        SUPPLY - TRANSFER_AMOUNT,
        "sender's mock-token should be burned by transfer amount"
    );
    assert_eq!(
        manager_after, 0,
        "manager must still hold zero tokens after burning-mode transfer"
    );

    let sent = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("transfer_sent", Duration::from_secs(15));
    assert!(sent.is_some(), "manager must emit transfer_sent within 15s");
}
