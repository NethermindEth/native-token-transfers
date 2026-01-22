use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext, decode_address_to_hex};

#[test]
#[ignore]
fn it_inbound_ratelimit_enforced() {
    let ctx = TestContext::new();

    // 1. Setup admin and transceiver identities and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("mock");

    let transceiver_identity = "transceiver_inbound_rl";
    let transceiver_addr = ctx.setup_identity(transceiver_identity);

    // 2. Deploy contracts
    println!("Deploying NTT manager...");
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Burning", 1);

    // 3. Configure manager
    println!("Registering transceiver...");
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_addr);

    let inbound_limit = 100u64;
    println!(
        "Registering remote peer (chain 2) with low inbound limit ({})...",
        inbound_limit
    );
    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        7,
        inbound_limit,
    );

    // 4. Prepare first inbound message (60 units) - should be consumed
    let recipient_identity = "recipient_rl";
    let recipient_addr = ctx.setup_identity(recipient_identity);
    let recipient_hex = decode_address_to_hex(&recipient_addr);

    let amount1 = 60u64;
    let full_message1_hex = ctx.create_ntt_message(&recipient_hex, amount1);

    println!("Submitting first attestation (60 units)...");
    let result1 = ctx.submit_attestation(
        transceiver_identity,
        &manager_id,
        &transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message1_hex,
    );

    println!("Result 1: {}", result1);
    // AttestationResult { approved: true, executed: true, queued: false }
    assert!(
        result1.contains("\"executed\":true"),
        "Expected first transfer to be executed"
    );
    assert!(
        result1.contains("\"queued\":false"),
        "Expected first transfer NOT to be queued"
    );

    // 5. Prepare second inbound message (50 units) - should be queued (60 + 50 = 110 > 100)
    let amount2 = 50u64;

    let full_message2_hex = ctx.create_ntt_message(&recipient_hex, amount2);

    println!("Submitting second attestation (50 units, should exceed limit)...");
    let result2 = ctx.submit_attestation(
        transceiver_identity,
        &manager_id,
        &transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message2_hex,
    );

    println!("Result 2: {}", result2);
    // AttestationResult { approved: true, executed: false, queued: true }
    assert!(
        result2.contains("\"executed\":false"),
        "Expected second transfer NOT to be executed"
    );
    assert!(
        result2.contains("\"queued\":true"),
        "Expected second transfer to be queued"
    );

    // 6. Verify first transfer balance
    let balance = ctx.get_balance(&token_id, &recipient_addr);
    assert_eq!(
        balance, amount1 as i128,
        "Recipient balance should only reflect the first transfer"
    );

    println!("Success: it_inbound_ratelimit_enforced passed!");
}
