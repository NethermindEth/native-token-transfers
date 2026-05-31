//! Manager-side peer check fires on inbound from a chain whose peer was never registered (surfaces as #36 through the transceiver wrapper).

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
    stack.register_transceiver_peer_only(&ctx, PEER_CHAIN, &PEER_ADDR);
    let recipient_addr = ctx.setup_identity("recipient_np");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_bytes32,
    }
}

/// Catches: the manager not rejecting an inbound transfer from a peer chain
/// that has no registered manager peer — would let arbitrary cross-chain
/// attackers credit our recipients. The transceiver wraps the manager error
/// as ManagerRejectedMessage (#36), so we assert #36; the fact that the call
/// errors at all proves the manager-side check fired.
#[test]
#[ignore]
fn inbound_from_unregistered_manager_peer_errors_36() {
    let f = setup();
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);

    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0x50; 32],
            sender: [0x60; 32],
            source_token: [0x70; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: 100_000_000,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: manager_bytes32,
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence: 0,
        guardian_secret: &f.ctx.guardian_secret,
    });

    let err = f
        .stack
        .try_receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex)
        .expect_err("receive_message must reject when manager has no peer for source chain");
    assert_eq!(
        err.code,
        Some(36),
        "expected ManagerRejectedMessage (#36, transceiver wrapper around manager's #50 PeerNotFound), got {:?}",
        err.code
    );
}
