//! Disabling a transceiver auto-reduces the threshold so the surviving transceiver can still execute alone.

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, stellar_addr_to_bytes32, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::TestContext;

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
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    let transceiver_two = stack.deploy_extra_transceiver(&ctx);
    stack.register_transceiver_addr(&ctx, &transceiver_two);
    stack.set_threshold(&ctx, 2);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_disable");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: the manager not auto-reducing the threshold when a transceiver is
/// disabled — which would either leave threshold=2 with only one enabled
/// transceiver (every inbound transfer permanently stuck below quorum) or
/// violate the threshold ≤ enabled-count invariant. After disabling the second
/// transceiver, the threshold must drop to 1 and a single attestation from the
/// surviving transceiver must execute.
#[test]
#[ignore]
fn disabling_transceiver_drops_threshold_and_executes_single_attestation() {
    let f = setup();

    f.stack.remove_transceiver(&f.ctx, &f.transceiver_two);

    let threshold = invoke(&f.ctx.admin_identity, &f.stack.manager, "get_threshold", &[]);
    assert_eq!(
        threshold.as_u64(),
        Some(1),
        "disabling one of two transceivers under threshold=2 must auto-drop threshold to 1"
    );

    let vaa = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xe0; 32],
            sender: [0xe1; 32],
            source_token: [0xe2; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: TRIMMED_AMOUNT,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: stellar_addr_to_bytes32(&f.stack.manager),
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence: 0,
        guardian_secret: &f.ctx.guardian_secret,
    });
    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa);

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr),
        EXPECTED_MINT,
        "after the threshold auto-drop, a single attestation must execute"
    );
}
