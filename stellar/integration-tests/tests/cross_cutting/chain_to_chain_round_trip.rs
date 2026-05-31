//! Round-trips one amount across the wire: outbound takes SAC custody, inbound (re-signed VAA from peer) releases it to a fresh recipient.

use integration_tests::cli::invoke;
use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::messages::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const ROUND_TRIP_AMOUNT: i128 = 10_000_000;
const TRIMMED_AMOUNT: u64 = 10_000_000;

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
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    let recipient_addr = ctx.setup_identity("c2c_recipient");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_addr,
        recipient_bytes32,
    }
}

/// Catches: any break in the outbound→inbound encoder/decoder symmetry —
/// what one leg of the manager emits, the other leg cannot consume. This
/// is the closest single-localnet analogue of EVM's `test_chainToChainBase`.
/// The Stellar manager takes SAC custody on outbound, then releases it back
/// via the inbound VAA path; net effect = the same amount has crossed the
/// bridge and returned to a *different* Stellar account, with the manager's
/// balance back to its initial state.
#[test]
#[ignore]
fn outbound_custody_then_inbound_release_round_trip() {
    let f = setup();
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    let manager_initial = f.stack.token_balance(&f.ctx, &f.stack.manager);
    let recipient_initial = f.stack.token_balance(&f.ctx, &f.recipient_addr);

    let peer_recipient = [0xbb; 32];
    invoke(
        &f.ctx.admin_identity,
        &f.stack.manager,
        "transfer",
        &[
            "--sender",
            &f.ctx.admin_address,
            "--amount",
            &ROUND_TRIP_AMOUNT.to_string(),
            "--recipient_chain",
            &PEER_CHAIN.to_string(),
            "--recipient",
            &hex::encode(peer_recipient),
            "--should_queue",
            "false",
        ],
    );

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.stack.manager) - manager_initial,
        ROUND_TRIP_AMOUNT,
        "outbound: manager must have taken SAC custody of the exact amount"
    );

    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xf0; 32],
            sender: PEER_ADDR,
            source_token: [0xf2; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: TRIMMED_AMOUNT,
            trimmed_decimals: 7,
        },
        source_manager: PEER_ADDR,
        recipient_manager: manager_bytes32,
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence: 0,
        guardian_secret: &f.ctx.guardian_secret,
    });
    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.recipient_addr) - recipient_initial,
        ROUND_TRIP_AMOUNT,
        "inbound: recipient must have received the unlocked amount"
    );
    assert_eq!(
        f.stack.token_balance(&f.ctx, &f.stack.manager),
        manager_initial,
        "round-trip: manager balance back to its initial state"
    );
}
