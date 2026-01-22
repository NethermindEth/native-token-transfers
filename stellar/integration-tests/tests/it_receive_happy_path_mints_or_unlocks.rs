use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext, decode_address_to_hex};

#[test]
#[ignore]
fn it_receive_happy_path_mints() {
    let ctx = TestContext::new();
    let setup = ctx.setup_default("mock", "Burning");

    // 4. Prepare inbound message
    let recipient_identity = "recipient_inbound";
    let recipient_addr = ctx.setup_identity(recipient_identity);
    let recipient_hex = decode_address_to_hex(&recipient_addr);

    let amount = 1000u64;
    let full_message_hex = ctx.create_ntt_message(&recipient_hex, amount);

    // 5. Submit attestation from transceiver
    println!("Submitting attestation from transceiver...");
    let result = ctx.submit_attestation(
        "transceiver",
        &setup.manager_id,
        &setup.transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    println!("Attestation result: {}", result);
    assert!(
        result.contains("true"),
        "Expected result to contain 'true' (approved/executed)"
    );

    // 6. Verify recipient balance
    println!("Verifying recipient balance...");
    let balance = ctx.get_balance(&setup.token_id, &recipient_addr);
    assert_eq!(
        balance, amount as i128,
        "Recipient did not receive the expected amount"
    );

    println!("Success: it_receive_happy_path_mints passed!");
}

#[test]
#[ignore]
fn it_receive_happy_path_unlocks() {
    let ctx = TestContext::new();
    let setup = ctx.setup_default("native", "Locking");

    // 3. Pre-fund manager contract
    // We fund the admin, then admin transfers some tokens to manager.
    let prefund_amount = 5000i128;
    println!("Pre-funding manager contract...");
    ctx.invoke(
        &ctx.admin_identity,
        &setup.token_id,
        "transfer",
        &[
            "--from",
            &setup.admin_addr,
            "--to",
            &setup.manager_id,
            "--amount",
            &prefund_amount.to_string(),
        ],
    );

    // 5. Prepare inbound message
    let recipient_identity = "recipient_unlock";
    let recipient_addr = ctx.setup_identity(recipient_identity);
    let recipient_hex = decode_address_to_hex(&recipient_addr);

    let amount = 1000u64;
    let full_message_hex = ctx.create_ntt_message(&recipient_hex, amount);

    // 6. Submit attestation from transceiver
    println!("Submitting attestation from transceiver...");
    let result = ctx.submit_attestation(
        "transceiver",
        &setup.manager_id,
        &setup.transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    println!("Attestation result: {}", result);
    assert!(result.contains("true"), "Expected result to contain 'true'");

    // 7. Verify recipient balance
    println!("Verifying recipient balance...");
    let balance = ctx.get_balance(&setup.token_id, &recipient_addr);
    // Initial balance from setup_identity/fund_account is 10000 XLM = 10000 * 10^7 = 100_000_000_000
    // Plus our 1000
    assert!(
        balance > amount as i128,
        "Recipient should have received the amount"
    );

    // Check manager balance decreased
    let manager_balance = ctx.get_balance(&setup.token_id, &setup.manager_id);
    assert_eq!(manager_balance, prefund_amount - amount as i128);

    println!("Success: it_receive_happy_path_unlocks passed!");
}
