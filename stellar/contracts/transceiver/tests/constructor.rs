#![cfg(test)]

#[path = "common/manager.rs"]
mod manager;
#[path = "common/wormhole.rs"]
mod wormhole;

use manager::MockNttManager;
use wormhole::MockWormholeCore;
use soroban_ntt_client::Mode;
use wormhole_soroban_client::hash_address;
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};
use stellar_ntt_transceiver::{TransceiverContract, TransceiverContractClient};

/// The constructor persists owner, manager, the manager id derived from it,
/// wormhole core, and version 1, and returns the exact `"wormhole"`
/// transceiver-type the Accountant compares byte-for-byte. Built inline so the
/// stored values can be checked against the exact inputs.
#[test]
fn constructor_stores_config_and_constants() {
    let env = Env::default();
    let owner = Address::generate(&env);
    let token = Address::generate(&env);
    let manager = env.register(MockNttManager, (token, Mode::Burning, 7u32));
    let core = env.register(MockWormholeCore, (0u64,));
    let t = TransceiverContractClient::new(&env, &env.register(TransceiverContract, (&owner, &manager, &core)));

    assert_eq!(t.get_owner(), Some(owner));
    assert_eq!(t.get_manager(), manager);
    assert_eq!(t.get_manager_id(), hash_address(&env, &manager));
    assert_eq!(t.get_wormhole_core(), core);
    assert_eq!(t.get_version(), 1);
    assert_eq!(t.get_transceiver_type(), Bytes::from_array(&env, b"wormhole"));
}
