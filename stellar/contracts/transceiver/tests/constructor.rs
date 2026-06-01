#![cfg(test)]

mod common;

use common::setup_transceiver;
use soroban_ntt_client::address_to_bytes32;
use soroban_sdk::{Bytes, Env};

/// The constructor persists owner, manager, the manager id derived from it,
/// wormhole core, and version 1, and returns the exact `"wormhole"`
/// transceiver-type the Accountant compares byte-for-byte.
#[test]
fn constructor_stores_config_and_constants() {
    let env = Env::default();
    let (owner, manager_id, transceiver, manager, core) = setup_transceiver(&env);

    assert_eq!(transceiver.get_owner(), Some(owner));
    assert_eq!(transceiver.get_manager(), manager.address);
    assert_eq!(manager_id, address_to_bytes32(&manager.address));
    assert_eq!(transceiver.get_wormhole_core(), core.address);
    assert_eq!(transceiver.get_version(), 1);
    assert_eq!(transceiver.get_transceiver_type(), Bytes::from_array(&env, b"wormhole"));
}
