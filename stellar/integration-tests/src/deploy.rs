use soroban_ntt_client::types::Mode;

use crate::cli;
use crate::ctx::TestContext;
use crate::vaa;

const GOVERNANCE_EMITTER_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000004";

pub struct WormholeCore;
pub struct MockToken;
pub struct Manager;
pub struct Transceiver;

impl WormholeCore {
    pub fn deploy(ctx: &TestContext, guardians: &[[u8; 20]]) -> String {
        let json = serde_json::to_string(
            &guardians.iter().map(hex::encode).collect::<Vec<_>>(),
        )
        .expect("encode guardian array");
        cli::deploy(
            &ctx.admin_identity,
            &ctx.wormhole_core_wasm_path,
            &[
                "--initial_guardians",
                &json,
                "--governance_emitter",
                GOVERNANCE_EMITTER_HEX,
            ],
        )
    }

    pub fn deploy_with_test_guardian(ctx: &TestContext) -> String {
        let addr = vaa::eth_address_from_privkey(&ctx.guardian_secret);
        Self::deploy(ctx, &[addr])
    }
}

impl MockToken {
    pub fn deploy(ctx: &TestContext, decimals: u32) -> String {
        let decimals_s = decimals.to_string();
        cli::deploy(
            &ctx.admin_identity,
            &ctx.mock_token_wasm_path,
            &["--decimals", &decimals_s],
        )
    }
}

impl Manager {
    pub fn deploy(
        ctx: &TestContext,
        owner: &str,
        token: &str,
        mode: Mode,
        chain_id: u32,
        outbound_limit: u64,
        rate_limit_duration: u64,
    ) -> String {
        let mode_str = match mode {
            Mode::Locking => "Locking",
            Mode::Burning => "Burning",
        };
        let chain_id_s = chain_id.to_string();
        let outbound_s = outbound_limit.to_string();
        let rate_s = rate_limit_duration.to_string();
        cli::deploy(
            &ctx.admin_identity,
            &ctx.manager_wasm_path,
            &[
                "--owner",
                owner,
                "--token",
                token,
                "--mode",
                mode_str,
                "--chain_id",
                &chain_id_s,
                "--outbound_limit",
                &outbound_s,
                "--rate_limit_duration",
                &rate_s,
            ],
        )
    }
}

impl Transceiver {
    pub fn deploy(
        ctx: &TestContext,
        owner: &str,
        manager: &str,
        wormhole_core: &str,
    ) -> String {
        cli::deploy(
            &ctx.admin_identity,
            &ctx.transceiver_wasm_path,
            &[
                "--owner",
                owner,
                "--manager",
                manager,
                "--wormhole_core",
                wormhole_core,
            ],
        )
    }
}
