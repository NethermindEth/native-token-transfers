use crate::common::wormhole::MockWormholeCoreClient;
use soroban_sdk::{Address, BytesN, Env};
use stellar_ntt_manager::ManagerContractClient;

/// Registers `recipient` on the manager's Wormhole core registry and returns its
/// canonical hash — the value an inbound message carries in `to`.
pub fn record_recipient(
    env: &Env,
    client: &ManagerContractClient<'_>,
    recipient: &Address,
) -> BytesN<32> {
    MockWormholeCoreClient::new(env, &client.get_wormhole_core()).record_address(recipient)
}
