//! End-to-end inbound burning happy path: signed VAA → Wormhole verify → transceiver decode → manager mint to recipient.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_hash};
use integration_tests::TestContext;

use crate::common::{
    peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS, STANDARD_RECEIVED,
    STANDARD_TRIMMED_AMOUNT,
};

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_addr: String,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    let recipient_addr = ctx.setup_identity("recipient");
    let recipient_bytes32 = stellar_addr_to_hash(&recipient_addr);
    stack.record_address(&ctx, &recipient_addr);
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

    let vaa_hex = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    let recipient_balance = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        recipient_balance, STANDARD_RECEIVED,
        "recipient must hold the untrimmed mint amount"
    );
}
