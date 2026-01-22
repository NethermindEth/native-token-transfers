use secp256k1::{Message, SecretKey, ecdsa::RecoverableSignature};
use serde_json::Value;
use std::{process::Command, thread::sleep, time::Duration};
use tiny_keccak::{Hasher, Keccak};

pub const SOURCE_CHAIN: u32 = 2;
pub const SOURCE_MANAGER: &str = "0000000000000000000000000000000000000000000000000000000000000002";
pub const SOURCE_TOKEN: &str = "0000000000000000000000000000000000000000000000000000000000000003";
pub const REMOTE_SENDER: &str = "0000000000000000000000000000000000000000000000000000000000000004";
pub const MESSAGE_ID: &str = "0000000000000000000000000000000000000000000000000000000000000005";

pub fn run(cmd: &mut Command) -> String {
    match try_run(cmd) {
        Ok(s) => s,
        Err(e) => panic!("command failed: {:?}\n{}", cmd, e),
    }
}

pub fn try_run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().expect("failed to spawn command");
    if out.status.success() {
        Ok(String::from_utf8(out.stdout).expect("stdout not utf8"))
    } else {
        Err(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

pub fn rpc_call(rpc_url: &str, body: &str) -> Value {
    let out = run(Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg(rpc_url)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body));
    let v: Value = serde_json::from_str(&out).expect("rpc response not json");
    if v["error"].is_object() {
        panic!("RPC error: {}\nRequest: {}", v["error"], body);
    }
    v
}

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

pub struct DefaultSetup {
    pub admin_addr: String,
    pub token_id: String,
    pub manager_id: String,
    pub transceiver_addr: String,
}

pub struct TestContext {
    pub network: String,
    pub admin_identity: String,
    pub rpc_url: String,
    pub manager_wasm_path: String,
    pub transceiver_wasm_path: String,
    pub mock_token_wasm_path: String,
    pub friendbot_url: String,
}

impl TestContext {
    pub fn new() -> Self {
        Self {
            network: std::env::var("STELLAR_NETWORK").unwrap_or_else(|_| "local".to_string()),
            admin_identity: std::env::var("STELLAR_IDENTITY").expect("STELLAR_IDENTITY not set"),
            rpc_url: std::env::var("SOROBAN_RPC_URL").expect("SOROBAN_RPC_URL not set"),
            manager_wasm_path: std::env::var("NTT_MANAGER_WASM_PATH")
                .expect("NTT_MANAGER_WASM_PATH not set"),
            transceiver_wasm_path: std::env::var("NTT_TRANSCEIVER_WASM_PATH")
                .expect("NTT_TRANSCEIVER_WASM_PATH not set"),
            mock_token_wasm_path: std::env::var("MOCK_TOKEN_WASM_PATH")
                .expect("MOCK_TOKEN_WASM_PATH not set"),
            friendbot_url: std::env::var("STELLAR_FRIENDBOT_URL")
                .expect("STELLAR_FRIENDBOT_URL not set"),
        }
    }

    pub fn setup_default(&self, token_type: &str, mode: &str) -> DefaultSetup {
        let (admin_addr, token_id) = self.setup_admin_and_token(token_type);
        let manager_id = self.deploy_manager_default(&admin_addr, &token_id, mode, 1);
        let transceiver_addr = self.setup_identity("transceiver");
        self.set_transceiver(&manager_id, &admin_addr, &transceiver_addr);
        self.set_peer(
            &manager_id,
            &admin_addr,
            SOURCE_CHAIN,
            SOURCE_MANAGER,
            7,
            1000000,
        );

        DefaultSetup {
            admin_addr,
            token_id,
            manager_id,
            transceiver_addr,
        }
    }

    pub fn create_ntt_message(&self, recipient_hex: &str, amount: u64) -> String {
        self.create_ntt_message_with_ids(recipient_hex, amount, MESSAGE_ID, REMOTE_SENDER)
    }

    pub fn create_ntt_message_with_ids(
        &self,
        recipient_hex: &str,
        amount: u64,
        message_id: &str,
        sender: &str,
    ) -> String {
        let payload_hex = ntt_payload_hex(
            amount,
            7, // default decimals
            SOURCE_TOKEN,
            recipient_hex,
            1, // to_chain (us)
        );

        ntt_manager_message_hex(message_id, sender, &payload_hex)
    }

    pub fn deploy_manager(
        &self,
        admin: &str,
        token: &str,
        mode: &str,
        chain_id: u32,
        outbound_limit: u64,
        rate_limit_duration: u64,
    ) -> String {
        let mode_val = match mode {
            "Locking" => "0",
            "Burning" => "1",
            _ => mode,
        };
        run(Command::new("stellar").args([
            "contract",
            "deploy",
            "--network",
            &self.network,
            "--source",
            &self.admin_identity,
            "--wasm",
            &self.manager_wasm_path,
            "--",
            "--admin",
            admin,
            "--token",
            token,
            "--mode",
            mode_val,
            "--chain_id",
            &chain_id.to_string(),
            "--outbound_limit",
            &outbound_limit.to_string(),
            "--rate_limit_duration",
            &rate_limit_duration.to_string(),
        ]))
        .trim()
        .to_string()
    }

    pub fn fund_account(&self, address: &str) {
        run(Command::new("curl")
            .arg("-s")
            .arg(format!("{}?addr={}", self.friendbot_url, address)));
    }

    pub fn get_identity_address(&self, name: &str) -> String {
        run(Command::new("stellar").args(["keys", "address", name]))
            .trim()
            .to_string()
    }

    pub fn setup_identity(&self, name: &str) -> String {
        let _ = Command::new("stellar").args(["keys", "rm", name]).output();
        run(Command::new("stellar").args(["keys", "generate", "--network", &self.network, name]));
        let addr = run(Command::new("stellar").args(["keys", "address", name]))
            .trim()
            .to_string();
        self.fund_account(&addr);
        addr
    }

    pub fn deploy_asset(&self, asset: &str) -> String {
        let id = run(Command::new("stellar").args([
            "contract",
            "id",
            "asset",
            "--asset",
            asset,
            "--network",
            &self.network,
        ]))
        .trim()
        .to_string();

        // Attempt to deploy, but ignore error if it already exists
        let _ = Command::new("stellar")
            .args([
                "contract",
                "asset",
                "deploy",
                "--asset",
                asset,
                "--network",
                &self.network,
                "--source",
                &self.admin_identity,
            ])
            .output();

        id
    }

    pub fn setup_admin_and_token(&self, token_type: &str) -> (String, String) {
        let admin_addr = self.get_identity_address(&self.admin_identity);
        self.fund_account(&admin_addr);

        let token_id = match token_type {
            "native" => self.deploy_asset("native"),
            "mock" => self.deploy_mock_token(),
            _ => panic!("invalid token type: {}", token_type),
        };

        (admin_addr, token_id)
    }

    pub fn set_transceiver(&self, manager_id: &str, admin_addr: &str, transceiver_addr: &str) {
        self.invoke(
            &self.admin_identity,
            manager_id,
            "set_transceiver",
            &["--admin", admin_addr, "--transceiver", transceiver_addr],
        );
    }

    pub fn set_peer(
        &self,
        manager_id: &str,
        admin_addr: &str,
        chain_id: u32,
        peer_address: &str,
        token_decimals: u32,
        inbound_limit: u64,
    ) {
        self.invoke(
            &self.admin_identity,
            manager_id,
            "set_peer",
            &[
                "--admin",
                admin_addr,
                "--chain_id",
                &chain_id.to_string(),
                "--peer_address",
                peer_address,
                "--token_decimals",
                &token_decimals.to_string(),
                "--inbound_limit",
                &inbound_limit.to_string(),
            ],
        );
    }

    pub fn deploy_manager_default(
        &self,
        admin_addr: &str,
        token_id: &str,
        mode: &str,
        chain_id: u32,
    ) -> String {
        self.deploy_manager(admin_addr, token_id, mode, chain_id, 1000000, 86400)
    }

    pub fn get_outbound_capacity(&self, manager_id: &str) -> u64 {
        let out = self.invoke(
            &self.admin_identity,
            manager_id,
            "get_outbound_capacity",
            &[],
        );
        out.trim()
            .trim_matches('"')
            .parse::<u64>()
            .expect("failed to parse capacity")
    }

    pub fn submit_attestation(
        &self,
        transceiver_identity: &str,
        manager_id: &str,
        transceiver_addr: &str,
        source_chain: u32,
        source_manager_hex: &str,
        message_hex: &str,
    ) -> String {
        self.invoke(
            transceiver_identity,
            manager_id,
            "attestation_received",
            &[
                "--transceiver",
                transceiver_addr,
                "--source_chain",
                &source_chain.to_string(),
                "--source_ntt_manager",
                source_manager_hex,
                "--payload",
                message_hex,
            ],
        )
    }

    pub fn try_submit_attestation(
        &self,
        transceiver_identity: &str,
        manager_id: &str,
        transceiver_addr: &str,
        source_chain: u32,
        source_manager_hex: &str,
        message_hex: &str,
    ) -> Result<String, String> {
        self.try_invoke(
            transceiver_identity,
            manager_id,
            "attestation_received",
            &[
                "--transceiver",
                transceiver_addr,
                "--source_chain",
                &source_chain.to_string(),
                "--source_ntt_manager",
                source_manager_hex,
                "--payload",
                message_hex,
            ],
        )
    }

    pub fn deploy_transceiver(&self) -> String {
        run(Command::new("stellar").args([
            "contract",
            "deploy",
            "--network",
            &self.network,
            "--source",
            &self.admin_identity,
            "--wasm",
            &self.transceiver_wasm_path,
        ]))
        .trim()
        .to_string()
    }

    pub fn deploy_mock_token(&self) -> String {
        run(Command::new("stellar").args([
            "contract",
            "deploy",
            "--network",
            &self.network,
            "--source",
            &self.admin_identity,
            "--wasm",
            &self.mock_token_wasm_path,
        ]))
        .trim()
        .to_string()
    }

    pub fn invoke(&self, source: &str, id: &str, func: &str, args: &[&str]) -> String {
        let mut cmd = Command::new("stellar");
        cmd.args([
            "contract",
            "invoke",
            "--network",
            &self.network,
            "--source",
            source,
            "--id",
            id,
            "--",
            func,
        ]);
        cmd.args(args);
        run(&mut cmd)
    }

    pub fn try_invoke(
        &self,
        source: &str,
        id: &str,
        func: &str,
        args: &[&str],
    ) -> Result<String, String> {
        let mut cmd = Command::new("stellar");
        cmd.args([
            "contract",
            "invoke",
            "--network",
            &self.network,
            "--source",
            source,
            "--id",
            id,
            "--",
            func,
        ]);
        cmd.args(args);
        try_run(&mut cmd)
    }

    pub fn get_balance(&self, asset_id: &str, address: &str) -> i128 {
        let out = self.invoke(
            &self.admin_identity,
            asset_id,
            "balance",
            &["--id", address],
        );
        out.trim()
            .trim_matches('"')
            .parse::<i128>()
            .expect("failed to parse balance")
    }
}

pub fn decode_address_to_hex(address: &str) -> String {
    let sk = stellar_strkey::Strkey::from_string(address).expect("invalid address");
    match sk {
        stellar_strkey::Strkey::PublicKeyEd25519(pk) => hex::encode(pk.0),
        stellar_strkey::Strkey::Contract(c) => hex::encode(c.0),
        _ => panic!("unsupported address type"),
    }
}

pub fn ntt_payload_hex(
    amount: u64,
    decimals: u8,
    source_token: &str, // hex
    to: &str,           // hex
    to_chain: u16,
) -> String {
    let mut payload = String::new();
    payload.push_str("994e5454"); // Prefix
    payload.push_str(&format!("{:02x}", decimals));
    payload.push_str(&format!("{:016x}", amount));
    payload.push_str(source_token);
    payload.push_str(to);
    payload.push_str(&format!("{:04x}", to_chain));
    payload
}

pub fn ntt_manager_message_hex(
    id: &str,      // hex
    sender: &str,  // hex
    payload: &str, // hex
) -> String {
    let mut msg = String::new();
    msg.push_str(id);
    msg.push_str(sender);
    let payload_len = (payload.len() / 2) as u16;
    msg.push_str(&format!("{:04x}", payload_len));
    msg.push_str(payload);
    msg
}

pub fn eth_address_from_privkey(privkey: &[u8; 32]) -> [u8; 20] {
    let sk = SecretKey::from_secret_bytes(*privkey).unwrap();
    let pk = secp256k1::PublicKey::from_secret_key(&sk);
    let pk_serialized = pk.serialize_uncompressed();
    let hash = keccak256(&pk_serialized[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

pub fn craft_governance_payload(action: u8, action_payload: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    let mut module = [0u8; 32];
    module[28..32].copy_from_slice(b"Core");
    payload.extend_from_slice(&module);
    payload.push(action);
    payload.extend_from_slice(&61u16.to_be_bytes()); // Chain ID: Stellar
    payload.extend_from_slice(action_payload);
    payload
}

pub fn assemble_vaa(
    guardian_set_index: u32,
    signatures: Vec<(u8, [u8; 64], u8)>,
    body: &[u8],
) -> Vec<u8> {
    let mut vaa = Vec::new();
    vaa.push(1); // Version
    vaa.extend_from_slice(&guardian_set_index.to_be_bytes());
    vaa.push(signatures.len() as u8);
    for (guardian_index, compact_sig, recovery_id) in signatures {
        vaa.push(guardian_index);
        vaa.extend_from_slice(&compact_sig);
        vaa.push(recovery_id);
    }
    vaa.extend_from_slice(body);
    vaa
}

pub fn craft_vaa_body(
    emitter_chain: u16,
    emitter_address: [u8; 32],
    nonce: u32,
    sequence: u64,
    payload: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&nonce.to_be_bytes());
    body.extend_from_slice(&emitter_chain.to_be_bytes());
    body.extend_from_slice(&emitter_address);
    body.extend_from_slice(&sequence.to_be_bytes());
    body.push(1);
    body.extend_from_slice(payload);
    body
}

pub fn sign_vaa_body(body: &[u8], privkey: [u8; 32]) -> (u8, [u8; 64]) {
    let body_hash = keccak256(&keccak256(body));
    let sk = SecretKey::from_secret_bytes(privkey).unwrap();
    let msg = Message::from_digest(body_hash);
    let sig = RecoverableSignature::sign_ecdsa_recoverable(msg, &sk);
    let (recid, compact) = sig.serialize_compact();
    (recid.to_u8(), compact)
}

pub fn find_event(rpc_url: &str, contract_id: &str, topic_filters: &[Vec<&str>]) -> bool {
    for _ in 0..15 {
        let latest = rpc_call(
            rpc_url,
            r#"{"jsonrpc":"2.0","id":1,"method":"getLatestLedger","params":{}}"#,
        );
        let latest_seq = latest["result"]["sequence"]
            .as_u64()
            .expect("latest ledger sequence missing");
        let start_ledger = latest_seq.saturating_sub(100).max(1);

        let events = rpc_call(
            rpc_url,
            &format!(
                r#"{{
                  "jsonrpc":"2.0","id":1,"method":"getEvents",
                  "params":{{
                    "startLedger": {start},
                    "endLedger": {end},
                    "filters": [{{"type":"contract","contractIds":["{cid}"]}}]
                  }}
                }}"#,
                start = start_ledger,
                end = latest_seq,
                cid = contract_id
            ),
        );

        let records = events["result"]["events"]
            .as_array()
            .expect("events result missing events array");
        for ev in records {
            let ev_str = ev.to_string();
            if topic_filters
                .iter()
                .all(|alternatives| alternatives.iter().any(|&s| ev_str.contains(s)))
            {
                return true;
            }
        }
        sleep(Duration::from_secs(1));
    }
    false
}
