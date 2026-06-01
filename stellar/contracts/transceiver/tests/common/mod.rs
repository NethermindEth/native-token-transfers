pub mod guardian;
pub mod manager;

use guardian::guardian_eth_address;
use manager::{MockManagerConfig, MockNttManager, MockNttManagerClient};
use soroban_ntt_client::Mode;
use soroban_sdk::{testutils::Address as _, vec, Address, BytesN, Env};
use stellar_ntt_transceiver::{TransceiverContract, TransceiverContractClient};
use wormhole_contract::Wormhole;
use wormhole_soroban_client::{WormholeClient, GOVERNANCE_EMITTER};

/// Deploys a transceiver over a mock NTT manager and the real in-process
/// Wormhole core, seeded with the suite's test guardian. Returns the owner, the
/// manager id the transceiver derived, and typed clients for the transceiver,
/// the manager, and the core.
///
/// Inbound tests sign VAAs the real core verifies (see [`vaa`]); the manager is
/// mocked because the transceiver's only job toward it is to forward the decoded
/// payload, which the mock records, and because rejection / query-failure must
/// be injected deterministically.
pub fn setup_transceiver<'a>(
    env: &Env,
) -> (
    Address,
    BytesN<32>,
    TransceiverContractClient<'a>,
    MockNttManagerClient<'a>,
    WormholeClient<'a>,
) {
    let owner = Address::generate(env);
    let token = Address::generate(env);

    let manager = env.register(
        MockNttManager,
        (MockManagerConfig {
            token,
            mode: Mode::Burning,
            decimals: 7,
            fail_attestation: false,
            fail_query: false,
        },),
    );

    let guardian = BytesN::from_array(env, &guardian_eth_address());
    let core = env.register(
        Wormhole,
        (vec![env, guardian], BytesN::from_array(env, &GOVERNANCE_EMITTER)),
    );

    let transceiver = TransceiverContractClient::new(
        env,
        &env.register(TransceiverContract, (&owner, &manager, &core)),
    );
    let manager_id = transceiver.get_manager_id();

    (
        owner,
        manager_id,
        transceiver,
        MockNttManagerClient::new(env, &manager),
        WormholeClient::new(env, &core),
    )
}
