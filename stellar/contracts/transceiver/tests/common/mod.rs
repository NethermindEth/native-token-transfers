pub mod manager;
pub mod wormhole;

use manager::MockNttManager;
use soroban_ntt_client::Mode;
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellar_ntt_transceiver::{TransceiverContract, TransceiverContractClient};
use wormhole::MockWormholeCore;

/// Deploys a transceiver over a mock NTT manager and a mock wormhole core, both
/// in their succeeding configuration. Returns the transceiver client; tests
/// derive the manager and core mock clients from its `get_manager` /
/// `get_wormhole_core` getters (via `MockNttManagerClient::new` etc.) and
/// configure them where a path needs it.
pub fn setup(env: &Env) -> TransceiverContractClient<'_> {
    let owner = Address::generate(env);
    let token = Address::generate(env);
    let manager = env.register(MockNttManager, (token, Mode::Burning, 7u32));
    let core = env.register(MockWormholeCore, (0u64,));
    TransceiverContractClient::new(env, &env.register(TransceiverContract, (&owner, &manager, &core)))
}
