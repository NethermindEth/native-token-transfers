use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext, find_event};

#[test]
#[ignore]
fn it_send_emits_expected_event() {
    let ctx = TestContext::new();

    // 1. Setup and fund admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 3. Deploy NTT manager
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
        1000000,
    );

    // 6. Execute transfer
    // We need some balance. Friendbot funds the account, so it should have balance of native asset.
    let amount = "1000";
    let recipient = "0000000000000000000000000000000000000000000000000000000000000003";

    println!("Executing transfer...");
    // Note: The admin identity must have authorized the transfer.
    // Soroban CLI handles this when --source is provided.
    ctx.invoke(
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

    // 7. Verify TransferSent event
    println!("Verifying TransferSent event...");
    // We use common base64 for Soroban Symbols:
    // "transfer" -> AAAADwAAAAh0cmFuc2Zlcg==
    // "send"     -> AAAADwAAAARzZW5k
    let found = find_event(
        &ctx.rpc_url,
        &manager_id,
        &[
            vec!["transfer", "AAAADwAAAAh0cmFuc2Zlcg=="],
            vec!["send", "AAAADwAAAARzZW5k"],
        ],
    );

    assert!(found, "TransferSent event not found");
    println!("Success: it_send_emits_expected_event passed!");
}
