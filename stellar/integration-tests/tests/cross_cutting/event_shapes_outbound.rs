//! Asserts the typed shape of the `transfer_sent` event — topic order and data field names.

use std::time::Duration;

use integration_tests::deploy::{Stack, StackOptions};
use integration_tests::TestContext;

use crate::common::{DUMMY_RECIPIENT, PEER_ADDR, PEER_CHAIN};

const SUPPLY: i128 = 100_000_000;
const TRANSFER_AMOUNT: i128 = 10_000_000;
const TRIMMED_AMOUNT: u64 = 10_000_000;

struct Fixture {
    ctx: TestContext,
    stack: Stack,
}

fn setup() -> Fixture {
    let ctx = TestContext::from_env();
    let stack = Stack::deploy(&ctx, &StackOptions::default());
    stack.register_transceiver(&ctx);
    stack.register_peer(&ctx, PEER_CHAIN, &PEER_ADDR, 8, u64::MAX);
    stack.mint_to(&ctx, &ctx.admin_address, SUPPLY);
    Fixture { ctx, stack }
}

/// Catches: changes to the `transfer_sent` event ABI — topic order or data
/// field names. Off-chain indexers key on these; even renaming a data field
/// like `amount` → `wire_amount` would silently break every integrator.
#[test]
#[ignore]
fn transfer_sent_event_carries_typed_topics_and_data_fields() {
    let f = setup();

    f.stack
        .transfer(&f.ctx, TRANSFER_AMOUNT, PEER_CHAIN, &DUMMY_RECIPIENT, false);

    let event = f
        .stack
        .manager_events(&f.ctx)
        .find_with_topic("transfer_sent", Duration::from_secs(15))
        .expect("transfer_sent must fire");

    assert_eq!(
        event.topic_u32(1),
        Some(PEER_CHAIN),
        "topic[1] must be recipient_chain"
    );
    assert_eq!(
        event.data_u64("amount"),
        Some(TRIMMED_AMOUNT),
        "data.amount must be the trimmed u64"
    );
    assert_eq!(
        event.data_u64("msg_sequence"),
        Some(1),
        "data.msg_sequence must be the manager's assigned sequence (1-indexed)"
    );
    assert!(
        event.data_field("recipient").is_some(),
        "data.recipient must be present"
    );
    assert!(
        event.data_field("fee").is_some(),
        "data.fee must be present"
    );
}
