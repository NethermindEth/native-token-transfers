//! Asserts the manager trims sub-canonical-decimal dust on outbound and reports the custodied amount in the `transfer_sent` event.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

use crate::common::{DUMMY_RECIPIENT, EVENT_TIMEOUT, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS};

const SOURCE_DECIMALS: u32 = 9;
const SUPPLY: i128 = 1_000_000_003;
const TRANSFER_AMOUNT: i128 = 1_000_000_003;
const EXPECTED_DUST: i128 = 3;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            token_decimals: SOURCE_DECIMALS,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: a regression in `TrimmedAmount::remove_dust` silently dropping
/// the dust on the sender (manager burns/locks the full pre-trim amount
/// instead of `amount - dust`). Asserts both the on-chain dust-retention
/// invariant and the custodied amount, in token decimals, carried in
/// `transfer_sent.amount`.
#[test]
#[ignore]
fn outbound_trims_to_canonical_decimals_keeps_dust_on_sender() {
    let f = setup();
    f.stack
        .transfer(&f.ctx, TRANSFER_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false);

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.ctx.admin_address),
        EXPECTED_DUST,
        "sender must keep the 3 raw units of dust below the 8-dec wire precision"
    );

    let event = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("transfer_sent", EVENT_TIMEOUT)
        .expect("transfer_sent must fire");
    assert_eq!(
        event.data_i128("amount"),
        Some(TRANSFER_AMOUNT - EXPECTED_DUST),
        "transfer_sent must report the custodied amount, not the raw pre-trim amount"
    );
}
