//! The transceiver has its own owner: a two-step transfer re-gates its owner-only set_peer onto the new identity.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

const NEW_OWNER_CHAIN: u32 = 2;
const OLD_OWNER_CHAIN: u32 = 3;
const EMITTER: [u8; 32] = [0xaa; 32];

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    new_owner_addr: String,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    let new_owner_addr = ctx.setup_identity("tcv_owner_two");
    Fixture {
        ctx,
        stack,
        new_owner_addr,
    }
}

/// Catches: the OZ two-step ownership being wired on the manager but not the
/// transceiver, or not re-gating the transceiver's owner-only set_peer. After
/// transferring the transceiver's ownership, only the new owner may set a peer;
/// the old owner (the deploy admin) must be rejected. Under live RPC auth this
/// is the only way to prove the swap took effect — mock_all_auths satisfies
/// anyone.
#[test]
#[ignore]
fn two_step_transfer_regates_transceiver_set_peer() {
    let f = setup();
    let live_until = f.ctx.current_ledger() + 1000;

    f.stack
        .transceiver_transfer_ownership(&f.ctx.admin_identity, &f.new_owner_addr, live_until);
    f.stack.transceiver_accept_ownership("tcv_owner_two");
    assert_eq!(f.stack.transceiver_owner(&f.ctx), f.new_owner_addr);

    f.stack
        .try_set_transceiver_peer("tcv_owner_two", &f.stack.transceiver, NEW_OWNER_CHAIN, &EMITTER)
        .expect("new owner must be able to set a transceiver peer");

    f.stack
        .try_set_transceiver_peer(
            &f.ctx.admin_identity,
            &f.stack.transceiver,
            OLD_OWNER_CHAIN,
            &EMITTER,
        )
        .expect_err("old owner must lose set_peer auth after the transfer");
}
