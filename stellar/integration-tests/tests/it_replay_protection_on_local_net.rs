use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext, decode_address_to_hex};

#[test]
#[ignore]
fn it_replay_protection_on_local_net() {
    let ctx = TestContext::new();

    // 1. Setup identities and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("mock");

    let transceiver1_identity = "transceiver1_replay";
    let transceiver1_addr = ctx.setup_identity(transceiver1_identity);

    let transceiver2_identity = "transceiver2_replay";
    let transceiver2_addr = ctx.setup_identity(transceiver2_identity);

    // 2. Deploy contracts
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Burning", 1);

    // 3. Configure manager
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver1_addr);
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver2_addr);

    ctx.set_peer(
        &manager_id,
        &admin_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        7,
        1000000,
    );

    // Set threshold to 2 to test partial attestation replay
    ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "set_threshold",
        &["--admin", &admin_addr, "--threshold", "2"],
    );

    // 4. Prepare message
    let recipient_addr = ctx.setup_identity("recipient_replay");
    let recipient_hex = decode_address_to_hex(&recipient_addr);

    let amount = 1000u64;
    let full_message_hex = ctx.create_ntt_message(&recipient_hex, amount);

    // 5. Scenario 1: Same transceiver attesting twice
    println!("Testing double attestation by same transceiver...");

    // First attestation from transceiver 1
    ctx.submit_attestation(
        transceiver1_identity,
        &manager_id,
        &transceiver1_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    // Second attestation from transceiver 1 (should fail with #81)
    let result = ctx.try_submit_attestation(
        transceiver1_identity,
        &manager_id,
        &transceiver1_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    match result {
        Err(e) if e.contains("Contract, #81") => {
            println!("Caught expected TransceiverAlreadyAttested error (#81)")
        }
        _ => panic!("Expected error #81, but got: {:?}", result),
    }

    // 6. Scenario 2: Attesting to already executed message
    println!("Testing attestation to already executed message...");

    // Meet threshold by attesting from transceiver 2
    ctx.submit_attestation(
        transceiver2_identity,
        &manager_id,
        &transceiver2_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    // Now it's executed. Try attesting AGAIN (even from a third "authorized" address if we had one, but transceiver 1 also works)
    // Actually, any further attestation should return #82 (TransferAlreadyRedeemed)

    let result2 = ctx.try_submit_attestation(
        transceiver1_identity,
        &manager_id,
        &transceiver1_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    match result2 {
        Err(e) if e.contains("Contract, #82") => {
            println!("Caught expected TransferAlreadyRedeemed error (#82)")
        }
        _ => panic!("Expected error #82, but got: {:?}", result2),
    }

    println!("Success: it_replay_protection_on_local_net passed!");
}
