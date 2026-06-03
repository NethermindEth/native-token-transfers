//! Asserts the typed shape of `message_attested_to` and `transfer_redeemed` events.

use std::time::Duration;

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_bytes32};
use integration_tests::TestContext;

use crate::common::{peer_inbound_vaa, PEER_ADDR, PEER_CHAIN, STANDARD_TRIMMED_AMOUNT};

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    recipient_bytes32: [u8; 32],
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    let recipient_addr = ctx.setup_identity("recipient_event");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);
    Fixture {
        ctx,
        stack,
        recipient_bytes32,
    }
}

/// Catches: changes to the `transfer_redeemed` or `message_attested_to`
/// event ABI — topic order, field presence, or transceiver-index value.
/// Off-chain indexers track these to reconstruct cross-chain transfer
/// state; a silent rename breaks every integrator.
#[test]
#[ignore]
fn inbound_emits_message_attested_to_and_transfer_redeemed() {
    let f = setup();

    let vaa_hex = build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        0,
    ));
    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    let attested = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("message_attested_to", Duration::from_secs(15))
        .expect("message_attested_to must fire");
    assert_eq!(
        attested.data_u32("index"),
        Some(0),
        "data.index must be the attesting transceiver's registry index (0 for the first)"
    );

    let redeemed = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("transfer_redeemed", Duration::from_secs(15))
        .expect("transfer_redeemed must fire");
    assert!(
        redeemed.topics.get(1).is_some(),
        "topic[1] must carry the digest"
    );
}
