//! Asserts the manager trims sub-canonical-decimal dust on outbound and emits the trimmed amount in the `transfer_sent` event.

use std::time::Duration;

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::events::EventQuery;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const PEER_DECIMALS: u32 = 8;
const SOURCE_DECIMALS: u32 = 9;
const SUPPLY: i128 = 1_000_000_003;
const TRANSFER_AMOUNT: i128 = 1_000_000_003;
const EXPECTED_DUST: i128 = 3;
const EXPECTED_TRIMMED_AMOUNT: u64 = 100_000_000;

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
            token_decimals: SOURCE_DECIMALS,
            outbound_limit: u64::MAX,
            rate_limit_duration: 1,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: a regression in `TrimmedAmount::remove_dust` letting non-canonical
/// decimals leak across the wire (off-chain integrators decode garbage), or
/// silently dropping the dust on the sender (manager burns/locks the full
/// pre-trim amount instead of `amount - dust`). Asserts both the on-chain
/// dust-retention invariant and the wire amount + decimals carried in
/// `transfer_sent.amount`.
#[test]
#[ignore]
fn outbound_trims_to_canonical_decimals_keeps_dust_on_sender() {
    let f = setup();
    let recipient = [0xbb; 32];

    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &TRANSFER_AMOUNT.to_string(),
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(recipient),
            "--should_queue",
            "false",
        ],
    );

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.ctx.admin_address),
        EXPECTED_DUST,
        "sender must keep the 3 raw units of dust below the 8-dec wire precision"
    );

    let event = EventQuery::new(&f.ctx, &f.stack.manager)
        .find_with_topic("transfer_sent", Duration::from_secs(15))
        .expect("transfer_sent must fire");
    assert_eq!(
        event.data_u64("amount"),
        Some(EXPECTED_TRIMMED_AMOUNT),
        "wire amount must be the trimmed value (8-dec canonical), not the raw pre-trim amount"
    );
}
