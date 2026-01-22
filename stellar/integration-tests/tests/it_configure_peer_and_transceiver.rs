use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext};

#[test]
#[ignore]
fn it_configure_peer_and_transceiver() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 4. Deploy and register a transceiver
    println!("Deploying transceiver...");
    let transceiver_id = ctx.deploy_transceiver();
    println!("Transceiver deployed at: {}", transceiver_id);

    println!("Registering transceiver...");
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_id);

    // 5. Verify transceiver state
    println!("Verifying transceiver state...");
    let count_query = ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "get_transceiver_count",
        &[],
    );
    assert!(
        count_query.trim().contains("1"),
        "Transceiver count mismatch, got {}",
        count_query
    );

    let bitmap_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_enabled_bitmap", &[]);
    assert!(
        bitmap_query.trim().contains("1"),
        "Enabled bitmap mismatch, got {}",
        bitmap_query
    );

    let threshold_query = ctx.invoke(&ctx.admin_identity, &manager_id, "get_threshold", &[]);
    assert!(
        threshold_query.trim().contains("1"),
        "Threshold mismatch (expected 1 after first transceiver), got {}",
        threshold_query
    );

    let info_query = ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "get_transceiver_info",
        &["--index", "0"],
    );
    assert!(
        info_query.contains(&transceiver_id),
        "Transceiver info mismatch, got {}",
        info_query
    );

    // 6. Register a remote peer
    println!("Registering remote peer...");
    let remote_token_decimals = 9;
    let inbound_limit = 500000;

    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        remote_token_decimals,
        inbound_limit,
    );

    // 7. Verify peer state
    println!("Verifying peer state...");
    let peer_query = ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "get_peer",
        &["--chain_id", &SOURCE_CHAIN.to_string()],
    );
    assert!(
        peer_query.contains(SOURCE_MANAGER),
        "Peer address mismatch, got {}",
        peer_query
    );
    assert!(
        peer_query.contains(&remote_token_decimals.to_string()),
        "Peer token decimals mismatch, got {}",
        peer_query
    );

    let limit_query = ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "get_inbound_limit_params",
        &["--chain_id", &SOURCE_CHAIN.to_string()],
    );
    assert!(
        limit_query.contains(&inbound_limit.to_string()),
        "Inbound limit mismatch, got {}",
        limit_query
    );

    println!("Success: it_configure_peer_and_transceiver passed!");
}
