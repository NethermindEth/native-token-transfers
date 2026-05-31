use integration_tests::cli::try_invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    pauser_addr: String,
    rogue_addr: String,
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
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, 1_000_000);

    let pauser_addr = ctx.setup_identity("pauser");
    let rogue_addr = ctx.setup_identity("rogue_pauser");

    stack.set_pauser_to(&ctx, &pauser_addr);

    Fixture {
        ctx,
        stack,
        pauser_addr,
        rogue_addr,
    }
}

/// Catches: Pausable wiring drift — pause/unpause auth split is non-obvious
/// (pauser may pause, but only owner may unpause). A regression that lets
/// transfers through while paused would silently bypass the kill switch;
/// a regression that lets non-pausers pause would be a denial-of-service
/// vector.
#[test]
#[ignore]
fn pause_blocks_transfer_unpause_is_owner_only() {
    let f = setup();
    let recipient = [0xbb; 32];

    f.stack.pause("pauser", &f.pauser_addr);
    assert!(f.stack.paused(&f.ctx));

    let err = try_invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            "100",
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            "false",
        ],
    )
    .expect_err("transfer must error while paused");
    assert_eq!(
        err.code,
        Some(1000),
        "expected EnforcedPause (#1000), got {:?}",
        err.code
    );

    f.stack
        .try_unpause("pauser", &f.pauser_addr)
        .expect_err("pauser must not be able to unpause; only owner unpauses");

    f.stack.unpause(&f.ctx.admin_identity, &f.ctx.admin_address);
    assert!(!f.stack.paused(&f.ctx));

    let rogue_err = f
        .stack
        .try_pause("rogue_pauser", &f.rogue_addr)
        .expect_err("non-pauser must not be able to pause");
    assert_eq!(
        rogue_err.code,
        Some(13),
        "expected NotAdminOrPauser (#13), got {:?}",
        rogue_err.code
    );
}
