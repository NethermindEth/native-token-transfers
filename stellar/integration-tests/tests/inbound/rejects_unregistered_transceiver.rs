//! Manager-side registry check fires when a rogue identity calls `attestation_received` directly.

use integration_tests::cli::try_invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{encode_ntt_manager_message, NttManagerMessageInputs};
use integration_tests::vaa::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    rogue_addr: String,
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
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    let rogue_addr = ctx.setup_identity("rogue_tcv");
    let recipient_addr = ctx.setup_identity("recipient_ut");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        rogue_addr,
        recipient_bytes32,
    }
}

/// Catches: a rogue account being able to submit `attestation_received` to
/// the manager as if it were a registered transceiver — would let any
/// funded identity forge attestations and (with threshold=1) drain the
/// manager. We bypass the transceiver entirely and assert the manager's
/// own registry check fires with #40.
#[test]
#[ignore]
fn rogue_caller_to_attestation_received_errors_40() {
    let f = setup();

    let payload = encode_ntt_manager_message(&NttManagerMessageInputs {
        id: [0x80; 32],
        sender: [0x90; 32],
        source_token: [0xa0; 32],
        recipient: f.recipient_bytes32,
        recipient_chain: f.ctx.stellar_chain_id,
        trimmed_amount: 100_000_000,
        trimmed_decimals: 8,
    });
    let payload_hex = hex::encode(&payload);
    let source_manager_hex = hex::encode(PEER_ADDR);

    let err = try_invoke(
        "rogue_tcv",
        &f.stack.manager,
        "attestation_received",
        &[
            "--transceiver",
            &f.rogue_addr,
            "--source_chain",
            &PEER_CHAIN.to_string(),
            "--source_ntt_manager",
            &source_manager_hex,
            "--payload",
            &payload_hex,
        ],
    )
    .expect_err("manager must reject attestation from an unregistered transceiver");
    assert_eq!(
        err.code,
        Some(40),
        "expected TransceiverNotRegistered (#40), got {:?}",
        err.code
    );
}
