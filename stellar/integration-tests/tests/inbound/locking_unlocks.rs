use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::vaa::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const PEER_DECIMALS: u32 = 8;
const PREFUND_XLM: i128 = 100_000_000;
const TRIMMED_AMOUNT: u64 = 100_000_000;
const EXPECTED_UNLOCK: i128 = 10_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_addr: String,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(
        &ctx,
        &StackOptions {
            mode: Mode::Locking,
            token_decimals: 7,
            outbound_limit: u64::MAX,
            rate_limit_duration: 1,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);

    let recipient_addr = ctx.setup_identity("recipient_unlock");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    invoke(
        &ctx.admin_identity,
        &stack.token,
        "transfer",
        &[
            "--from",
            &ctx.admin_address,
            "--to",
            &stack.manager,
            "--amount",
            &PREFUND_XLM.to_string(),
        ],
    );

    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: locking-mode inbound break where the manager fails to release SAC
/// custody to the recipient (would freeze cross-chain inbound on the canonical
/// chain), or releases the wrong amount due to misapplied untrim math.
#[test]
#[ignore]
fn inbound_locking_unlocks_recipient() {
    let f = setup();
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    let manager_before = f.stack.token_balance(&f.ctx, &f.stack.manager);
    let recipient_before = f.stack.token_balance(&f.ctx, &f.recipient_addr);

    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0x11; 32],
            sender: [0x21; 32],
            source_token: [0x31; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: TRIMMED_AMOUNT,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: manager_bytes32,
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence: 0,
        guardian_secret: &f.ctx.guardian_secret,
    });

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    let manager_after = f.stack.token_balance(&f.ctx, &f.stack.manager);
    let recipient_after = f.stack.token_balance(&f.ctx, &f.recipient_addr);

    assert_eq!(
        manager_before - manager_after,
        EXPECTED_UNLOCK,
        "manager XLM must drop by the unlocked amount"
    );
    assert_eq!(
        recipient_after - recipient_before,
        EXPECTED_UNLOCK,
        "recipient XLM must rise by the unlocked amount"
    );
}
