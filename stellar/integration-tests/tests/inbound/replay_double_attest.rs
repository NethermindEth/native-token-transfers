use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::vaa::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_bytes32: [u8; 32],
    recipient_addr: String,
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
    let tcv2 = stack.deploy_extra_transceiver(&ctx);
    stack.register_transceiver_addr(&ctx, &tcv2);
    stack.set_threshold(&ctx, 2);

    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &tcv2, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_dbl");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

fn build_vaa(f: &Fixture, sequence: u64) -> String {
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xb0; 32],
            sender: [0xb1; 32],
            source_token: [0xb2; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: 100_000_000,
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

/// Catches: a single transceiver being able to double-attest the same
/// message digest — would let one compromised transceiver unilaterally
/// meet any threshold and drain the manager. Setup uses threshold=2 and
/// two distinct VAAs with same NTT payload but different wormhole
/// sequence (so transceiver-level replay protection lets both through),
/// then verifies the manager rejects the second with #81 (wrapped as
/// transceiver's ManagerRejectedMessage #36) and never executes.
#[test]
#[ignore]
fn same_transceiver_double_attest_errors_and_never_executes() {
    let f = setup();

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &build_vaa(&f, 0));

    let after_first = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_first, 0,
        "1/2 attestations must not execute under threshold=2"
    );

    let err = f
        .stack
        .try_receive_message(&f.ctx, &f.stack.transceiver, &build_vaa(&f, 1))
        .expect_err("second submission from the same transceiver must be rejected");
    assert_eq!(
        err.code,
        Some(36),
        "expected ManagerRejectedMessage (#36, wraps manager's #81), got {:?}",
        err.code
    );

    let after_second = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_second, 0,
        "double-attest must not execute the transfer"
    );
}
