//! End-to-end outbound locking happy path: sender debits to manager SAC custody, `transfer_sent` fires.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

use crate::common::{DUMMY_RECIPIENT, EVENT_TIMEOUT, PEER_ADDR, PEER_CHAIN};

const TRANSFER_AMOUNT: i128 = 100_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
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
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    Fixture { ctx, stack }
}

/// Catches: SAC custody breakage where the manager emits TransferSent without
/// actually pulling tokens from the sender — silent loss / orphaned outbound
/// transfers across the bridge.
#[test]
#[ignore]
fn outbound_locking_debits_sender_credits_manager_contract() {
    let f = setup();

    let sender_before = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    let manager_before = f.stack.token_balance(&f.ctx, &f.stack.manager);

    f.stack
        .transfer(&f.ctx, TRANSFER_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false);

    let sender_after = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    let manager_after = f.stack.token_balance(&f.ctx, &f.stack.manager);

    assert_eq!(
        manager_after - manager_before,
        TRANSFER_AMOUNT,
        "manager XLM must rise by exactly the transfer amount"
    );
    assert!(
        sender_before - sender_after >= TRANSFER_AMOUNT,
        "sender XLM must drop by at least the transfer amount (extra = gas); \
         got drop = {} for transfer = {}",
        sender_before - sender_after,
        TRANSFER_AMOUNT
    );

    let sent = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("transfer_sent", EVENT_TIMEOUT);
    assert!(sent.is_some(), "manager must emit transfer_sent within 15s");
}
