use integration_tests::cli::try_invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
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
            mode: Mode::Burning,
            token_decimals: 8,
            outbound_limit: OUTBOUND_LIMIT,
            rate_limit_duration: 10,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

fn transfer(f: &Fixture, amount: i128, should_queue: bool) -> Result<(), u32> {
    let recipient = [0xbb; 32];
    try_invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &amount.to_string(),
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            if should_queue { "true" } else { "false" },
        ],
    )
    .map(|_| ())
    .map_err(|e| e.code.expect("error must have a contract code"))
}

/// Catches: should_queue=false path leaking custody — sender debited despite
/// rate-limit rejection (silent loss). Also: error code drift from 62 to
/// something generic, hiding rate-limit hits from upstream observability.
#[test]
#[ignore]
fn outbound_over_capacity_no_queue_refunds_and_errors_62() {
    let f = setup();

    transfer(&f, FIRST_AMOUNT, false).expect("first transfer should succeed");
    let after_first = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(after_first, SUPPLY - FIRST_AMOUNT);

    let err = transfer(&f, SECOND_AMOUNT, false)
        .expect_err("second transfer should fail with rate-limit reject");
    assert_eq!(err, 62, "expected TransferExceedsRateLimit (#62), got #{err}");

    let after_reject = f.stack.token_balance(&f.ctx, &f.ctx.admin_address);
    assert_eq!(
        after_reject, after_first,
        "rejected transfer must not consume sender balance"
    );
}
