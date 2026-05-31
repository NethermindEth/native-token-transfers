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
const TRIMMED_AMOUNT: u64 = 100_000_000;
const EXPECTED_MINT: i128 = 10_000_000;

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
            mode: Mode::Burning,
            token_decimals: 7,
            outbound_limit: u64::MAX,
            rate_limit_duration: 1,
        },
    );
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    let recipient_addr = ctx.setup_identity("recipient");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: end-to-end inbound break where a signed Wormhole VAA from a
/// registered peer fails to mint to the recipient — or mints the wrong
/// untrimmed amount because decimal conversion silently rounds. Exercises
/// the full pipeline: wormhole-core verify_vaa, transceiver decode +
/// emitter check, manager attestation + execute.
#[test]
#[ignore]
fn inbound_burning_mints_recipient() {
    let f = setup();
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);

    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0x10; 32],
            sender: [0x20; 32],
            source_token: [0x30; 32],
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

    invoke(
        &f.ctx.admin_identity,
        &f.stack.transceiver,
        "receive_message",
        &["--vaa_bytes", &vaa_hex],
    );

    let recipient_balance = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        recipient_balance, EXPECTED_MINT,
        "recipient must hold the untrimmed mint amount"
    );
}
