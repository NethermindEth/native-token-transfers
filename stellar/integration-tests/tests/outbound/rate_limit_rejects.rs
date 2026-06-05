//! Outbound transfer over rate-limit capacity with `should_queue=false` is rejected without leaking sender custody.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

use crate::common::{DUMMY_RECIPIENT, PEER_ADDR, PEER_CHAIN};

const SUPPLY: i128 = 1_000;
const OUTBOUND_LIMIT: u64 = 100;
const FIRST_AMOUNT: i128 = 50;
const SECOND_AMOUNT: i128 = 75;

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
            rate_limit_duration: 10,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: should_queue=false path leaking custody — sender debited despite
/// rate-limit rejection (silent loss). Also: error code drift from 62 to
/// something generic, hiding rate-limit hits from upstream observability.
#[test]
#[ignore]
fn outbound_over_capacity_no_queue_refunds_and_errors_62() {
    let f = setup();

    f.stack
        .try_transfer(&f.ctx, FIRST_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false)
        .expect("first transfer should succeed");
    let after_first = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(after_first, SUPPLY - FIRST_AMOUNT);

    let err = f
        .stack
        .try_transfer(&f.ctx, SECOND_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false)
        .expect_err("second transfer should fail with rate-limit reject");
    assert_eq!(
        err.code,
        Some(62),
        "expected TransferExceedsRateLimit (#62), got {:?}",
        err.code
    );

    let after_reject = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(
        after_reject, after_first,
        "rejected transfer must not consume sender balance"
    );
}
