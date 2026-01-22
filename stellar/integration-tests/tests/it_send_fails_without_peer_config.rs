use integration_tests::{SOURCE_CHAIN, TestContext};

#[test]
#[ignore]
fn it_send_fails_without_peer_config() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 4. Attempt transfer to chain 2 (not registered as peer)
    let amount = "1000";
    let recipient = "0000000000000000000000000000000000000000000000000000000000000003";

    println!("Executing transfer to unconfigured chain (expecting failure)...");

    let result = ctx.try_invoke(
        &ctx.admin_identity,
        &manager_id,
        "transfer",
        &[
            "--sender",
            &admin_addr,
            "--amount",
            amount,
            "--recipient_chain",
            &SOURCE_CHAIN.to_string(),
            "--recipient",
            recipient,
            "--should_queue",
            "false",
        ],
    );

    // 5. Verify failure with PeerNotFound (50)
    match result {
        Ok(out) => panic!(
            "Transfer should have failed, but succeeded with output: {}",
            out
        ),
        Err(err) => {
            println!("Caught expected error: {}", err);
            // Error(Contract, #50) is the typical format for contract errors in Stellar CLI
            assert!(
                err.contains("Error(Contract, #50)") || err.contains("50"),
                "Error should indicate PeerNotFound (50), but got: {}",
                err
            );
        }
    }

    println!("Success: it_send_fails_without_peer_config passed!");
}
