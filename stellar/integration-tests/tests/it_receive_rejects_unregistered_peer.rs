use integration_tests::{SOURCE_CHAIN, SOURCE_MANAGER, TestContext, decode_address_to_hex};

#[test]
#[ignore]
fn it_receive_rejects_unregistered_peer() {
    let ctx = TestContext::new();

    // 1. Setup admin and transceiver identities and deploy token
    let (admin_addr, token_id) = ctx.setup_admin_and_token("native");

    let transceiver_identity = "transceiver_reject";
    let transceiver_addr = ctx.setup_identity(transceiver_identity);

    // 2. Deploy NTT manager
    let manager_id = ctx.deploy_manager_default(&admin_addr, &token_id, "Locking", 1);

    // 3. Register transceiver
    ctx.set_transceiver(&manager_id, &admin_addr, &transceiver_addr);

    // 4. Prepare message from UNREGISTERED chain 2
    let recipient_addr = ctx.get_identity_address(&ctx.admin_identity);
    let recipient_hex = decode_address_to_hex(&recipient_addr);

    let amount = 1000u64;
    let full_message_hex = ctx.create_ntt_message(&recipient_hex, amount);

    // 5. Submit attestation from transceiver for unregistered peer
    println!("Submitting attestation from unregistered peer (chain 2)...");
    let result = ctx.try_submit_attestation(
        transceiver_identity,
        &manager_id,
        &transceiver_addr,
        SOURCE_CHAIN,
        SOURCE_MANAGER,
        &full_message_hex,
    );

    // 6. Verify rejection
    match result {
        Err(e) => {
            println!("Caught expected error: {}", e);
            assert!(
                e.contains("Error(Contract, #50)"),
                "Expected PeerNotFound error (50), got: {}",
                e
            );
        }
        Ok(s) => panic!(
            "Expected attestation to be rejected, but it succeeded with: {}",
            s
        ),
    }

    println!("Success: it_receive_rejects_unregistered_peer passed!");
}
