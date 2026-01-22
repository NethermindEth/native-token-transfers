use integration_tests::TestContext;

#[test]
#[ignore]
fn testnet_initialize_and_query_manager() {
    let ctx = TestContext::new();

    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // Deploy the manager contract
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // Query state
    assert!(
        ctx.invoke(&ctx.admin_identity, &manager_id, "get_admin", &[])
            .contains(&admin_addr)
    );

    assert!(
        ctx.invoke(&ctx.admin_identity, &manager_id, "get_token", &[])
            .contains(&token_id)
    );

    assert!(
        ctx.invoke(&ctx.admin_identity, &manager_id, "get_mode", &[])
            .contains("0")
    );

    assert!(
        ctx.invoke(&ctx.admin_identity, &manager_id, "get_chain_id", &[])
            .contains("1")
    );
}
