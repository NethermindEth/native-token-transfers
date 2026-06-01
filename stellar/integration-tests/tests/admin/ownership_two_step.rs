//! Asserts the two-step ownership transfer actually re-gates owner-only methods on the new identity.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    new_owner_addr: String,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    let new_owner_addr = ctx.setup_identity("owner_two");
    Fixture {
        ctx,
        stack,
        new_owner_addr,
    }
}

/// Catches: the two-step ownership transfer not actually swapping the owner
/// or not re-gating owner-only methods on the new identity. Without signed
/// auth this is invisible (mock_all_auths satisfies anyone); only the live
/// RPC auth tree proves the swap took effect.
#[test]
#[ignore]
fn two_step_transfer_swaps_owner_auth() {
    let f = setup();
    let live_until = f.ctx.current_ledger() + 1000;

    f.stack.transfer_ownership(
        &f.ctx.admin_identity,
        &f.new_owner_addr,
        live_until,
    );
    f.stack.accept_ownership("owner_two");

    assert_eq!(f.stack.owner(&f.ctx), f.new_owner_addr);

    f.stack.set_outbound_limit("owner_two", 999);

    f.stack
        .try_set_outbound_limit(&f.ctx.admin_identity, 888)
        .expect_err("old owner must lose privileged-setter auth after ownership transfer");
}
