use std::env;
use std::process::Command;

use crate::cli;

pub struct TestContext {
    pub network: String,
    pub network_passphrase: String,
    pub admin_identity: String,
    pub admin_address: String,
    pub rpc_url: String,
    pub friendbot_url: String,
    pub manager_wasm_path: String,
    pub transceiver_wasm_path: String,
    pub mock_token_wasm_path: String,
    pub wormhole_core_wasm_path: String,
    pub guardian_secret: [u8; 32],
    pub stellar_chain_id: u32,
}

impl TestContext {
    pub fn from_env() -> Self {
        let network =
            env::var("STELLAR_NETWORK").expect("STELLAR_NETWORK not set");
        let admin_identity =
            env::var("STELLAR_IDENTITY").expect("STELLAR_IDENTITY not set");
        let admin_address = cli::run(&["keys", "address", &admin_identity])
            .trim()
            .to_string();
        Self {
            network,
            network_passphrase: env::var("STELLAR_NETWORK_PASSPHRASE")
                .expect("STELLAR_NETWORK_PASSPHRASE not set"),
            admin_identity,
            admin_address,
            rpc_url: env::var("SOROBAN_RPC_URL").expect("SOROBAN_RPC_URL not set"),
            friendbot_url: env::var("STELLAR_FRIENDBOT_URL")
                .expect("STELLAR_FRIENDBOT_URL not set"),
            manager_wasm_path: env::var("NTT_MANAGER_WASM_PATH")
                .expect("NTT_MANAGER_WASM_PATH not set"),
            transceiver_wasm_path: env::var("NTT_TRANSCEIVER_WASM_PATH")
                .expect("NTT_TRANSCEIVER_WASM_PATH not set"),
            mock_token_wasm_path: env::var("MOCK_TOKEN_WASM_PATH")
                .expect("MOCK_TOKEN_WASM_PATH not set"),
            wormhole_core_wasm_path: env::var("WORMHOLE_CORE_WASM_PATH")
                .expect("WORMHOLE_CORE_WASM_PATH not set"),
            guardian_secret: decode_guardian_secret(),
            stellar_chain_id: env::var("STELLAR_CHAIN_ID")
                .expect("STELLAR_CHAIN_ID not set")
                .parse()
                .expect("STELLAR_CHAIN_ID not u32"),
        }
    }

    pub fn network_add(&self) {
        let _ = cli::try_run(&["network", "rm", &self.network]);
        cli::run(&[
            "network",
            "add",
            &self.network,
            "--rpc-url",
            &self.rpc_url,
            "--network-passphrase",
            &self.network_passphrase,
        ]);
    }

    pub fn setup_identity(&self, name: &str) -> String {
        if cli::try_run(&["keys", "address", name]).is_err() {
            cli::run(&["keys", "generate", "--network", &self.network, name]);
        }
        let addr = cli::run(&["keys", "address", name]).trim().to_string();
        self.friendbot(&addr);
        addr
    }

    pub fn native_sac(&self) -> String {
        let _ = cli::try_run(&[
            "contract",
            "asset",
            "deploy",
            "--asset",
            "native",
            "--source",
            &self.admin_identity,
            "--network",
            &self.network,
        ]);
        cli::run(&[
            "contract",
            "id",
            "asset",
            "--asset",
            "native",
            "--network",
            &self.network,
        ])
        .trim()
        .to_string()
    }

    pub fn friendbot(&self, addr: &str) {
        let url = format!("{}?addr={}", self.friendbot_url, addr);
        let out = Command::new("curl")
            .args(["-s", &url])
            .output()
            .expect("failed to spawn curl");
        if !out.status.success() {
            panic!(
                "friendbot failed for {addr}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

fn decode_guardian_secret() -> [u8; 32] {
    let hex_str =
        env::var("GUARDIAN_SECRET_HEX").expect("GUARDIAN_SECRET_HEX not set");
    let bytes =
        hex::decode(hex_str.trim()).expect("GUARDIAN_SECRET_HEX not hex");
    if bytes.len() != 32 {
        panic!(
            "GUARDIAN_SECRET_HEX must be 32 bytes, got {}",
            bytes.len()
        );
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}
