use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext};

#[test]
#[ignore]
fn it_receive_rejects_wrong_transceiver() {
    let ctx = TestContext::new();

    // 1. Setup admin identity and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    // 2. Deploy NTT manager
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 3. Setup an unregistered transceiver identity
    let rogue_transceiver_identity = "rogue_transceiver";
    let rogue_transceiver_addr = ctx.setup_identity(rogue_transceiver_identity);

    // 4. Try to call attestation_received with unregistered transceiver
    println!("Executing attestation_received with unregistered transceiver (expecting failure)...");

    let dummy_payload = "00"; // Minimum hex payload

    let result = ctx.try_submit_attestation(
        rogue_transceiver_identity,
        &manager_id,
        &rogue_transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        dummy_payload,
    );

    // 5. Verify failure
    match result {
        Err(e) => {
            println!("Caught expected error: {}", e);
            assert!(
                e.contains("Error(Contract, #40)"),
                "Expected TransceiverNotRegistered error (#40), got: {}",
                e
            );
        }
        Ok(s) => panic!(
            "Expected attestation_received to fail, but it succeeded with: {}",
            s
        ),
    }

    // 6. Test registered but disabled transceiver
    println!("Testing registered but disabled transceiver...");
    let transceiver_identity = "disabled_transceiver";
    let transceiver_addr = ctx.setup_identity(transceiver_identity);

    // Register it
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_addr);

    // We need another transceiver to be able to remove this one (cannot remove last transceiver)
    let t2_addr = ctx.setup_identity("t2");
    ctx.set_transceiver(&manager_id, &admin_addr, &t2_addr);

    // Disable the first one
    ctx.invoke(
        &ctx.admin_identity,
        &manager_id,
        "remove_transceiver",
        &["--admin", &admin_addr, "--transceiver", &transceiver_addr],
    );

    // Try to call attestation_received with disabled transceiver
    println!("Executing attestation_received with disabled transceiver (expecting failure)...");
    let result = ctx.try_submit_attestation(
        transceiver_identity,
        &manager_id,
        &transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        dummy_payload,
    );

    match result {
        Err(e) => {
            println!("Caught expected error: {}", e);
            assert!(
                e.contains("Error(Contract, #80)"),
                "Expected TransceiverNotEnabled error (#80), got: {}",
                e
            );
        }
        Ok(s) => panic!(
            "Expected attestation_received to fail, but it succeeded with: {}",
            s
        ),
    }

    println!("Success: it_receive_rejects_wrong_transceiver passed!");
}
