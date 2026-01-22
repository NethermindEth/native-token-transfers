use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext};

#[test]
#[ignore]
fn it_send_updates_balances_correctly() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager in Locking mode
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 4. Deploy and register transceiver
    let transceiver_id = ctx.deploy_transceiver();
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_id);

    // 5. Register peer (chain 2)
    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        7,
        10000000000,
    );

    // 6. Setup a user identity and fund it
    let user_name = "test_user_balance";
    let user_addr = ctx.setup_identity(user_name);

    // 7. Record initial balances
    let user_initial_balance = ctx.get_balance(&token_id, &user_addr);
    let manager_initial_balance = ctx.get_balance(&token_id, &manager_id);

    println!("User initial balance: {}", user_initial_balance);
    println!("Manager initial balance: {}", manager_initial_balance);

    // 8. Execute transfer
    let transfer_amount: i128 = 100_0000000; // 100 XLM
    let recipient = "0000000000000000000000000000000000000000000000000000000000000003";

    println!("Executing transfer of {} from user...", transfer_amount);
    ctx.invoke(
        user_name,
        &manager_id,
        "transfer",
        &[
            "--sender",
            &user_addr,
            "--amount",
            &transfer_amount.to_string(),
            "--recipient_chain",
            &SOURCE_CHAIN.to_string(),
            "--recipient",
            recipient,
            "--should_queue",
            "false",
        ],
    );

    // 9. Record final balances
    let user_final_balance = ctx.get_balance(&token_id, &user_addr);
    let manager_final_balance = ctx.get_balance(&token_id, &manager_id);

    println!("User final balance: {}", user_final_balance);
    println!("Manager final balance: {}", manager_final_balance);

    // 10. Assertions
    // User balance should be reduced by at least transfer_amount (plus transaction fees)
    assert!(
        user_final_balance <= user_initial_balance - transfer_amount,
        "User balance did not decrease by at least the expected amount. Initial: {}, Final: {}, Expected reduction: {}",
        user_initial_balance,
        user_final_balance,
        transfer_amount
    );

    // Manager balance should be increased by exactly transfer_amount (escrowed)
    assert_eq!(
        manager_final_balance,
        manager_initial_balance + transfer_amount,
        "Manager balance did not increase by the expected amount"
    );

    println!("Success: it_send_updates_balances_correctly passed!");
}
