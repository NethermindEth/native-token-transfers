use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext};

#[test]
#[ignore]
fn it_send_updates_balances_correctly_burning() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy mock token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("mock");
    println!("Mock token deployed at: {}", token_id);

    // 3. Deploy NTT manager in Burning mode
    println!("Deploying NTT manager in Burning mode...");
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Burning", 1);
    println!("NTT manager deployed at: {}", manager_id);

    // 4. Register mock transceiver
    let transceiver_id = ctx.deploy_transceiver();
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_id);

    // 5. Register peer (chain 2)
    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        7,
        1000000,
    );

    // 6. Setup test user and mint tokens
    let user_name = "user_burning";
    let user_addr = ctx.setup_identity(user_name);
    let initial_balance: i128 = 100_000_000_000; // 10,000 tokens

    println!("Minting tokens to user...");
    ctx.invoke(
        &ctx.admin_identity,
        &token_id,
        "mint",
        &["--to", &user_addr, "--amount", &initial_balance.to_string()],
    );

    let user_balance_before = ctx.get_balance(&token_id, &user_addr);
    assert_eq!(user_balance_before, initial_balance);
    println!("User initial balance: {}", user_balance_before);

    // 7. Execute transfer from user
    let transfer_amount: i128 = 1_000_000_000; // 100 tokens
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

    // 8. Verify balances
    let user_balance_after = ctx.get_balance(&token_id, &user_addr);
    println!("User final balance: {}", user_balance_after);

    // In Burning mode, tokens are burned from the user.
    // The balance should be exactly (initial - transfer_amount).
    // Note: Since we use a mock token and not the native asset, there are no gas fees deducted FROM THE TOKEN BALANCE.
    // (Stellar gas fees are deducted from native asset balance, but here we check our custom mock token balance).
    assert_eq!(
        user_balance_after,
        initial_balance - transfer_amount,
        "User balance mismatch after burn"
    );

    // Verify manager balance (should be 0 in Burning mode as tokens are burned, not escrowed)
    let manager_balance = ctx.get_balance(&token_id, &manager_id);
    println!("Manager final balance: {}", manager_balance);
    assert_eq!(
        manager_balance, 0,
        "Manager should have 0 balance in Burning mode"
    );

    println!("Success: it_send_updates_balances_correctly_burning passed!");
}
