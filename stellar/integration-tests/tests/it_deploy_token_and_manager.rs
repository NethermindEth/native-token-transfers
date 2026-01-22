use integration_tests::TestContext;

#[test]
#[ignore]
fn it_deploy_token_and_manager() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");
    println!("Token deployed at: {}", token_id);

    // 3. Deploy NTT manager
    println!("Deploying NTT manager...");
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);
    println!("NTT manager deployed at: {}", manager_id);

    // 4. Verify deployment by querying manager state
    println!("Verifying manager state...");

    let admin_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_admin", &[]);
    assert!(
        admin_query.contains(&admin_addr),
        "Admin mismatch: expected {}, got {}",
        admin_addr,
        admin_query
    );

    let token_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_token", &[]);
    assert!(
        token_query.contains(&token_id),
        "Token mismatch: expected {}, got {}",
        token_id,
        token_query
    );

    let mode_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_mode", &[]);
    assert!(
        mode_query.contains("0"), // 0 = Locking
        "Mode mismatch: expected 0 (Locking), got {}",
        mode_query
    );

    let chain_id_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_chain_id", &[]);
    assert!(
        chain_id_query.contains("1"),
        "Chain ID mismatch: expected 1, got {}",
        chain_id_query
    );

    println!("Success: it_deploy_token_and_manager passed!");
}
