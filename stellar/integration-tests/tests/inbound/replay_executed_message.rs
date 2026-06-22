//! Post-execution replay via a second transceiver is idempotent — no double-mint.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{build_inbound_vaa_hex, stellar_addr_to_hash};
use integration_tests::TestContext;

use crate::common::{
    peer_inbound_vaa, EVENT_TIMEOUT, PEER_ADDR, PEER_CHAIN, STANDARD_RECEIVED,
    STANDARD_TRIMMED_AMOUNT,
};

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

    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_exe");
    let recipient_bytes32 = stellar_addr_to_hash(&recipient_addr);
    stack.record_address(&ctx, &recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

fn build_vaa(f: &Fixture, sequence: u64) -> String {
    build_inbound_vaa_hex(&peer_inbound_vaa(
        &f.ctx,
        &f.stack.manager,
        f.recipient_bytes32,
        STANDARD_TRIMMED_AMOUNT,
        sequence,
    ))
}

/// Catches: an executed inbound message being replayed via a second
/// transceiver causing double-mint — critical security bug. The Soroban
/// manager's idempotency: after a digest is executed, further attestations
/// for that digest record the vote, emit `message_already_executed`, and
/// return Ok without re-executing the transfer. We verify by running the
/// full second VAA through a *different* transceiver (so transceiver-level
/// replay does not fire) and asserting the recipient's balance is unchanged.
#[test]
#[ignore]
fn second_transceiver_replay_after_execution_is_idempotent() {
    let f = setup();

    f.stack.receive_message(&f.ctx, &f.stack.transceiver, &build_vaa(&f, 0));

    let after_first = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_first, STANDARD_RECEIVED,
        "threshold=1: first attestation must execute the mint"
    );

    f.stack.receive_message(&f.ctx, &f.transceiver_two, &build_vaa(&f, 1));

    let after_second = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_second, STANDARD_RECEIVED,
        "second VAA via a different transceiver must NOT mint again"
    );

    let replayed = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("message_already_executed", EVENT_TIMEOUT);
    assert!(
        replayed.is_some(),
        "replaying an executed digest must emit message_already_executed, \
         confirming the idempotency path ran"
    );
}
