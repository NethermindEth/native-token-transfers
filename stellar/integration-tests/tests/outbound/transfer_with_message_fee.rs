//! A nonzero Wormhole message fee, set by governance on the real core, is quoted
//! and collected on both outbound paths: a transfer and a broadcast.

use integration_tests::cli;
use integration_tests::deploy::{parse_i128, Stack, StackOptions};
use integration_tests::messages::build_set_message_fee_vaa_hex;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

use crate::common::{DUMMY_RECIPIENT, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS};

/// Core message fee in stroops.
const FEE: u64 = 1000;
const TRANSFER_AMOUNT: i128 = 100_000_000;

/// Catches: a fee the core cannot collect (no allowance, or the XLM contract of
/// another network), which fails every transfer and broadcast once governance
/// sets a fee.
#[test]
#[ignore]
fn transfer_and_broadcast_pay_the_core_message_fee() {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            mode: Mode::Locking,
            ..Default::default()
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    let fee_vaa = build_set_message_fee_vaa_hex(FEE, &ctx.guardian_secret);
    cli::invoke(
        &ctx.admin_identity,
        &stack.wormhole_core,
        "submit_set_message_fee",
        &["--vaa_bytes", &fee_vaa],
    );

    let chain = PEER_CHAIN.to_string();
    let quotes = cli::invoke(
        &ctx.admin_identity,
        &stack.manager,
        "quote_delivery_price",
        &["--recipient_chain", &chain],
    );
    assert_eq!(parse_i128(&quotes[0]["fee"]), i128::from(FEE));

    // The token is native XLM, so the fee must not come out of the manager's
    // custody, and the core's token balance is the fees it collected.
    stack.transfer(&ctx, TRANSFER_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false);
    assert_eq!(stack.token_balance(&ctx, &stack.manager), TRANSFER_AMOUNT);
    assert_eq!(
        stack.token_balance(&ctx, &stack.wormhole_core),
        i128::from(FEE)
    );

    cli::invoke(
        &ctx.admin_identity,
        &stack.transceiver,
        "broadcast_peer",
        &["--payer", &ctx.admin_address, "--chain_id", &chain],
    );
    assert_eq!(
        stack.token_balance(&ctx, &stack.wormhole_core),
        2 * i128::from(FEE)
    );
    assert_eq!(stack.token_balance(&ctx, &stack.transceiver), 0);
}
