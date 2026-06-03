//! The transceiver's inbound rejection barriers: a structurally-valid VAA from
//! the registered peer is rejected — with the exact pre-dispatch error — when
//! verification fails, the emitter is wrong, the recipient manager is wrong, or
//! the peer is disabled. Each fires inside the transceiver before manager
//! dispatch, so the assertions are the unwrapped codes, not the #36 wrapper.

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::messages::{
    build_inbound_vaa_hex, build_inbound_vaa_hex_tampered_signature, InboundVaaInputs,
};
use integration_tests::TestContext;

use crate::common::{
    peer_inbound_vaa, DUMMY_RECIPIENT, PEER_ADDR, PEER_CHAIN, PEER_DECIMALS,
    STANDARD_TRIMMED_AMOUNT,
};

const ROGUE_EMITTER: [u8; 32] = [0xbb; 32];
const WRONG_MANAGER: [u8; 32] = [0xcc; 32];

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, PEER_DECIMALS, u64::MAX);
    Fixture { ctx, stack }
}

/// A valid inbound VAA from the standard peer. Each test perturbs exactly
/// one aspect of this baseline (the signature, the emitter, the recipient
/// manager, or the peer's enabled state) via struct-update syntax.
fn valid_inputs(f: &Fixture) -> InboundVaaInputs<'_> {
    peer_inbound_vaa(&f.ctx, &f.stack.manager, DUMMY_RECIPIENT, STANDARD_TRIMMED_AMOUNT, 0)
}

fn assert_receive_rejects(f: &Fixture, vaa_hex: &str, code: u32) {
    let err = f
        .stack
        .try_receive_message(&f.ctx, &f.stack.transceiver, vaa_hex)
        .expect_err("receive_message must reject");
    assert_eq!(err.code, Some(code), "expected #{code}, got {:?}", err.code);
}

/// Catches the highest-severity bug: trusting a VAA whose guardian signature
/// does not verify. One signature byte is flipped, so the real Wormhole core
/// must reject it with WormholeVerificationFailed (#20). The unit suite mocks
/// the core, so real secp256k1 verification failure is only exercised here.
#[test]
#[ignore]
fn tampered_signature_rejected_20() {
    let f = setup();
    let vaa = build_inbound_vaa_hex_tampered_signature(&valid_inputs(&f));
    assert_receive_rejects(&f, &vaa, 20);
}

/// Catches a validly-signed VAA from an emitter that is not the registered
/// peer — a compromised emitter on the source chain spoofing transfers.
/// Verification passes; only the emitter differs, so the transceiver's
/// peer-check must reject with UnexpectedEmitter (#35).
#[test]
#[ignore]
fn unexpected_emitter_rejected_35() {
    let f = setup();
    let vaa = build_inbound_vaa_hex(&InboundVaaInputs {
        emitter_address: ROGUE_EMITTER,
        ..valid_inputs(&f)
    });
    assert_receive_rejects(&f, &vaa, 35);
}

/// Catches cross-manager confusion: a VAA addressed to a different NTT manager
/// being consumed here. Signature and emitter are valid; only the recipient
/// manager differs, so it must reject with UnexpectedRecipientManager (#33).
#[test]
#[ignore]
fn unexpected_recipient_manager_rejected_33() {
    let f = setup();
    let vaa = build_inbound_vaa_hex(&InboundVaaInputs {
        recipient_manager: WRONG_MANAGER,
        ..valid_inputs(&f)
    });
    assert_receive_rejects(&f, &vaa, 33);
}

/// Catches the set_peer_enabled kill-switch being callable but ineffective:
/// after the owner disables the peer, an otherwise-valid signed VAA from it
/// must be rejected with PeerDisabled (#14).
#[test]
#[ignore]
fn disabled_peer_rejected_14() {
    let f = setup();
    f.stack
        .set_transceiver_peer_enabled(&f.ctx, &f.stack.transceiver, PEER_CHAIN, false);
    let vaa = build_inbound_vaa_hex(&valid_inputs(&f));
    assert_receive_rejects(&f, &vaa, 14);
}
