//! Manager rejects an outbound transfer to a chain with no registered peer.

use integration_tests::cli::try_invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const UNREGISTERED_CHAIN: u32 = 99;
const SUPPLY: i128 = 1_000;
const TRANSFER_AMOUNT: i128 = 100;

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
            token_decimals: 7,
            outbound_limit: u64::MAX,
            rate_limit_duration: 1,
        },
    );
    stack.register_transceiver(&ctx);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: PeerNotFound error drifting from #50 to a generic code, or
/// silently routing to a black-hole peer address (untraceable lost tokens).
#[test]
#[ignore]
fn transfer_to_unconfigured_chain_errors_50() {
    let f = setup();

    let recipient = [0xbb; 32];
    let err = try_invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &TRANSFER_AMOUNT.to_string(),
            "--recipient_chain",
            &UNREGISTERED_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            "false",
        ],
    )
    .expect_err("transfer to unconfigured chain must error");
    assert_eq!(
        err.code,
        Some(50),
        "expected PeerNotFound (#50), got {:?}",
        err.code
    );
}
