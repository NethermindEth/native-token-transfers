//! Asserts the typed shape of `message_attested_to` and `transfer_redeemed` events.

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
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);

    let vaa_hex = build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xe0; 32],
            sender: [0xe1; 32],
            source_token: [0xe2; 32],
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
    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &vaa_hex);

    let attested = EventQuery::new(&f.ctx, &f.stack.manager)
        .find_with_topic("message_attested_to", Duration::from_secs(15))
        .expect("message_attested_to must fire");
    assert_eq!(
        attested.data_u32("index"),
        Some(0),
        "data.index must be the attesting transceiver's registry index (0 for the first)"
    );

    let redeemed = EventQuery::new(&f.ctx, &f.stack.manager)
        .find_with_topic("transfer_redeemed", Duration::from_secs(15))
        .expect("transfer_redeemed must fire");
    assert!(
        redeemed.topics.get(1).is_some(),
        "topic[1] must carry the digest"
    );
}
