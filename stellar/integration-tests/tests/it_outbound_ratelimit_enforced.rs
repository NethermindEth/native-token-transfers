use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext};

#[test]
#[ignore]
fn it_outbound_ratelimit_enforced() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager with low limit
    // limit = 100, duration = 86400 (1 day)
    let manager_id = ctx.deploy_manager(
        &admin_addr,
        &token_id,
        "Locking",
        1,     // our chain_id
        100,   // outbound_limit
        86400, // rate_limit_duration
    );

    // 4. Register transceiver
    let transceiver_id = ctx.deploy_transceiver();
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_id);

    // 5. Register peer (chain 2)
    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        7,
        1000,
    );

    // 6. First transfer: 60 tokens (within limit of 100)
    println!("Executing first transfer (60 tokens)...");
    ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "transfer",
        &[
            "--sender",
            &admin_addr,
            "--amount",
            "60",
            "--recipient_chain",
            &SOURCE_CHAIN.to_string(),
            "--recipient",
            "0000000000000000000000000000000000000000000000000000000000000003",
            "--should_queue",
            "false",
        ],
    );

    // 7. Verify capacity
    let capacity = ctx.get_outbound_capacity(&manager_id);
    println!("Remaining capacity: {}", capacity);
    // Capacity should be around 40 (100 - 60). Refill is negligible.
    assert!(capacity <= 40, "Capacity should be <= 40, got {}", capacity);

    // 8. Second transfer: 50 tokens (exceeds remaining capacity of 40)
    println!("Executing second transfer (50 tokens, should fail)...");
    let result = ctx.try_invoke(
        &ctx.admin_identity,
        &manager_id,
        "transfer",
        &[
            "--sender",
            &admin_addr,
            "--amount",
            "50",
            "--recipient_chain",
            &SOURCE_CHAIN.to_string(),
            "--recipient",
            "0000000000000000000000000000000000000000000000000000000000000003",
            "--should_queue",
            "false",
        ],
    );

    match result {
        Err(e) => {
            println!("Caught expected error: {}", e);
            assert!(
                e.contains("Error(Contract, #62)"),
                "Expected Error(Contract, #62), got {}",
                e
            );
        }
        Ok(s) => panic!(
            "Transfer should have failed due to rate limit, but succeeded: {}",
            s
        ),
    }

    println!("Success: it_outbound_ratelimit_enforced passed!");
}
