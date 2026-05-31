use std::time::Duration;

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::events::EventQuery;
use integration_tests::messages::{
    build_inbound_vaa_hex, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::vaa::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const TRIMMED_AMOUNT: u64 = 100_000_000;
const EXPECTED_MINT: i128 = 10_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    transceiver_two: String,
    recipient_addr: String,
    recipient_bytes32: [u8; 32],
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
    let transceiver_two = stack.deploy_extra_transceiver(&ctx);
    stack.register_transceiver_addr(&ctx, &transceiver_two);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_thresh");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

fn build_vaa(f: &Fixture, sequence: u64) -> String {
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xd0; 32],
            sender: [0xd1; 32],
            source_token: [0xd2; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: TRIMMED_AMOUNT,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: manager_bytes32,
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence,
        guardian_secret: &f.ctx.guardian_secret,
    })
}

/// Catches: a regression in threshold enforcement — either a single
/// attestation executing despite threshold=2 (no quorum at all), or a
/// quorum-met second attestation failing to execute (live-locked).
/// Also catches: `set_threshold` not emitting the `threshold_changed`
/// event, which off-chain monitors rely on.
#[test]
#[ignore]
fn raise_threshold_requires_quorum_before_execution() {
    let f = setup();

    f.stack.set_threshold(&f.ctx, 2);
    let event = EventQuery::new(&f.ctx, &f.stack.manager)
        .find_with_topic("threshold_changed", Duration::from_secs(10));
    assert!(event.is_some(), "set_threshold(2) must emit threshold_changed");

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &build_vaa(&f, 0));
    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        0,
        "single attestation must not execute under threshold=2"
    );

    f.stack.receive_message(&f.ctx, &f.transceiver_two, &build_vaa(&f, 1));
    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        EXPECTED_MINT,
        "quorum met, recipient must be minted"
    );
}
