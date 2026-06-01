//! Asserts the manager refuses to remove its only registered transceiver.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    Fixture { ctx, stack }
}

/// Catches: a regression that lets the owner remove the only registered
/// transceiver — bricks the manager (no outbound or inbound possible
/// until a new transceiver is registered, and threshold>0 invariant means
/// the contract is stuck in a non-recoverable state without governance).
#[test]
#[ignore]
fn remove_only_transceiver_errors_46() {
    let f = setup();

    let err = f
        .stack
        .try_remove_transceiver(&f.ctx, &f.stack.transceiver)
        .expect_err("removing the last transceiver must error");
    assert_eq!(
        err.code,
        Some(46),
        "expected CannotDisableLastTransceiver (#46), got {:?}",
        err.code
    );
}
