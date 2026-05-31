//! Post-execution replay via a second transceiver is idempotent — no double-mint.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::messages::stellar_addr_to_bytes32;
use integration_tests::TestContext;
use soroban_ntt_client::types::Mode;

const PEER_CHAIN: u32 = 2;
const PEER_ADDR: [u8; 32] = [0xaa; 32];
const TRIMMED_AMOUNT: u64 = 100_000_000;
const EXPECTED_MINT: i128 = 10_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
    transceiver_two: String,
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
    let transceiver_two = stack.deploy_extra_transceiver(&ctx);
    stack.register_transceiver_addr(&ctx, &transceiver_two);

    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.set_transceiver_peer(&ctx, &transceiver_two, PEER_CHAIN, &PEER_ADDR);

    let recipient_addr = ctx.setup_identity("recipient_exe");
    let recipient_bytes32 = stellar_addr_to_bytes32(&recipient_addr);

    Fixture {
        ctx,
        stack,
        transceiver_two,
        recipient_addr,
        recipient_bytes32,
    }
}

fn build_vaa(f: &Fixture, sequence: u64) -> String {
    let manager_bytes32 = stellar_addr_to_bytes32(&f.stack.manager);
    build_inbound_vaa_hex(&InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0xc0; 32],
            sender: [0xc1; 32],
            source_token: [0xc2; 32],
            recipient: f.recipient_bytes32,
            recipient_chain: f.ctx.stellar_chain_id,
            trimmed_amount: TRIMMED_AMOUNT,
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
        after_first, EXPECTED_MINT,
        "threshold=1: first attestation must execute the mint"
    );

    f.stack.receive_message(&f.ctx, &f.transceiver_two, &build_vaa(&f, 1));

    let after_second = f.stack.token_balance(&f.ctx, &f.recipient_addr);
    assert_eq!(
        after_second, EXPECTED_MINT,
        "second VAA via a different transceiver must NOT mint again"
    );
}
