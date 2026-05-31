use serde_json::Value;
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
            Mode::Locking => "0",
            Mode::Burning => "1",
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

pub struct StackOptions {
    pub mode: Mode,
    pub token_decimals: u32,
    pub outbound_limit: u64,
    pub rate_limit_duration: u64,
}

pub struct Stack {
    pub mode: Mode,
    pub wormhole_core: String,
    pub token: String,
    pub manager: String,
    pub transceiver: String,
}

impl Stack {
    pub fn deploy(ctx: &TestContext, opts: &StackOptions) -> Self {
        let wormhole_core = WormholeCore::deploy_with_test_guardian(ctx);
        let token = match opts.mode {
            Mode::Locking => ctx.native_sac(),
            Mode::Burning => MockToken::deploy(ctx, opts.token_decimals),
        };
        let manager = Manager::deploy(
            ctx,
            &ctx.admin_address,
            &token,
            opts.mode,
            ctx.stellar_chain_id,
            opts.outbound_limit,
            opts.rate_limit_duration,
        );
        let transceiver = Transceiver::deploy(
            ctx,
            &ctx.admin_address,
            &manager,
            &wormhole_core,
        );
        Self {
            mode: opts.mode,
            wormhole_core,
            token,
            manager,
            transceiver,
        }
    }

    pub fn register_transceiver(&self, ctx: &TestContext) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_transceiver",
            &["--transceiver", &self.transceiver],
        );
    }

    pub fn register_peer(
        &self,
        ctx: &TestContext,
        chain_id: u32,
        peer_addr: &[u8; 32],
        peer_decimals: u32,
        inbound_limit: u64,
    ) {
        let peer_hex = hex::encode(peer_addr);
        let chain_id_s = chain_id.to_string();
        let decimals_s = peer_decimals.to_string();
        let limit_s = inbound_limit.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_peer",
            &[
                "--chain_id",
                &chain_id_s,
                "--peer_address",
                &peer_hex,
                "--token_decimals",
                &decimals_s,
                "--inbound_limit",
                &limit_s,
            ],
        );
        cli::invoke(
            &ctx.admin_identity,
            &self.transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &peer_hex],
        );
    }

    pub fn token_balance(&self, ctx: &TestContext, account: &str) -> i128 {
        let v = cli::invoke(
            &ctx.admin_identity,
            &self.token,
            "balance",
            &["--id", account],
        );
        parse_i128(&v)
    }

    pub fn mint_to(&self, ctx: &TestContext, recipient: &str, amount: i128) {
        if matches!(self.mode, Mode::Locking) {
            panic!("mint_to only works in Burning mode; Locking uses friendbot-funded XLM");
        }
        let amount_s = amount.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.token,
            "mint",
            &["--to", recipient, "--amount", &amount_s],
        );
    }

    pub fn register_transceiver_addr(&self, ctx: &TestContext, transceiver: &str) {
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_transceiver",
            &["--transceiver", transceiver],
        );
    }

    pub fn register_transceiver_peer_only(
        &self,
        ctx: &TestContext,
        chain_id: u32,
        peer_addr: &[u8; 32],
    ) {
        let peer_hex = hex::encode(peer_addr);
        let chain_id_s = chain_id.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &peer_hex],
        );
    }

    pub fn set_transceiver_peer(
        &self,
        ctx: &TestContext,
        transceiver: &str,
        chain_id: u32,
        peer_addr: &[u8; 32],
    ) {
        let peer_hex = hex::encode(peer_addr);
        let chain_id_s = chain_id.to_string();
        cli::invoke(
            &ctx.admin_identity,
            transceiver,
            "set_peer",
            &["--chain_id", &chain_id_s, "--emitter", &peer_hex],
        );
    }

    pub fn set_threshold(&self, ctx: &TestContext, threshold: u32) {
        let t = threshold.to_string();
        cli::invoke(
            &ctx.admin_identity,
            &self.manager,
            "set_threshold",
            &["--threshold", &t],
        );
    }

    pub fn deploy_extra_transceiver(&self, ctx: &TestContext) -> String {
        Transceiver::deploy(
            ctx,
            &ctx.admin_address,
            &self.manager,
            &self.wormhole_core,
        )
    }
}

pub fn parse_i128(v: &Value) -> i128 {
    if let Some(s) = v.as_str() {
        return s.parse().expect("i128 not parseable");
    }
    if let Some(n) = v.as_i64() {
        return i128::from(n);
    }
    if let Some(n) = v.as_u64() {
        return i128::from(n);
    }
    panic!("cannot parse i128 from JSON value: {v}");
}
