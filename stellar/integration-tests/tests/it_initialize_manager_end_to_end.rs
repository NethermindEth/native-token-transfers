use integration_tests::TestContext;

#[test]
#[ignore]
fn it_initialize_manager_end_to_end() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager
    // params: admin, token, mode, chain_id, outbound_limit, rate_limit_duration
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 4. Verify configuration via getters

    // get_admin
    let admin_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_admin", &[]);
    assert!(admin_query.contains(&admin_addr), "Admin mismatch");

    // get_token
    let token_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_token", &[]);
    assert!(token_query.contains(&token_id), "Token mismatch");

    // get_mode
    let mode_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_mode", &[]);
    assert!(
        mode_query.contains("0"),
        "Mode mismatch (expected 0/Locking)"
    );

    // get_chain_id
    let chain_id_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_chain_id", &[]);
    assert!(chain_id_query.contains("1"), "Chain ID mismatch");

    // token_decimals
    let decimals_query = ctx.invoke(&ctx.admin_identity, &manager_id, "token_decimals", &[]);
    assert!(
        decimals_query.contains("7"),
        "Token decimals mismatch (expected 7 for native)"
    );

    // get_threshold
    let threshold_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_threshold", &[]);
    assert!(
        threshold_query.contains("0"),
        "Threshold mismatch (expected 0 initially)"
    );

    // get_version
    let version_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_version", &[]);
    assert!(version_query.contains("1"), "Version mismatch");

    // get_rate_limit_duration
    let duration_query = ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "get_rate_limit_duration",
        &[],
    );
    assert!(
        duration_query.contains("86400"),
        "Rate limit duration mismatch"
    );

    // get_pauser (should be null/None)
    let pauser_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_pauser", &[]);
    assert!(
        pauser_query.contains("null"),
        "Pauser should be null initially"
    );
}
