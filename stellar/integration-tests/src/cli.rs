//! Typed wrapper around the `stellar` command-line tool.
//!
//! All harness interaction with the localnet goes through this module:
//! [`run`] / [`try_run`] for arbitrary subcommands (key generation, network
//! registration, etc.), [`invoke`] / [`try_invoke`] for contract function
//! calls (stdout parsed as JSON), and [`deploy`] for contract deployment
//! (stdout's last C-strkey is the new contract id).
//!
//! Errors carry the contract code parsed out of the CLI's `Error(Contract,
//! #N)` text via [`CliError`], so tests can assert exact codes without
//! string matching.

use serde_json::Value;
use std::process::Command;

/// Failed `stellar` invocation. `code` is the contract error number parsed
/// from the CLI's stderr, when present; `raw` is the full stdout + stderr
/// for diagnostic messages.
pub struct CliError {
    pub code: Option<u32>,
    pub raw: String,
}

/// Runs `stellar <args>` and returns stdout. Panics if the call fails.
pub fn run(args: &[&str]) -> String {
    try_run(args).unwrap_or_else(|e| panic!("stellar CLI failed:\n{}", e.raw))
}

/// Runs `stellar <args>` and returns stdout, or a [`CliError`] with the
/// parsed contract code on failure.
pub fn try_run(args: &[&str]) -> Result<String, CliError> {
    let out = Command::new("stellar")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn stellar: {e}"));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let raw = format!("stdout:\n{stdout}\nstderr:\n{stderr}");
        Err(CliError {
            code: extract_error_code(&raw),
            raw,
        })
    }
}

/// Invokes `func` on contract `id` signed by `source`, parsing stdout as
/// JSON. Reads `STELLAR_NETWORK` from the environment. Panics on failure.
pub fn invoke(source: &str, id: &str, func: &str, fn_args: &[&str]) -> Value {
    try_invoke(source, id, func, fn_args)
        .unwrap_or_else(|e| panic!("invoke {func} failed:\n{}", e.raw))
}

/// Like [`invoke`] but returns a [`CliError`] on failure so the caller can
/// assert against the contract error code.
pub fn try_invoke(
    source: &str,
    id: &str,
    func: &str,
    fn_args: &[&str],
) -> Result<Value, CliError> {
    let network =
        std::env::var("STELLAR_NETWORK").expect("STELLAR_NETWORK not set");
    let mut args: Vec<&str> = vec![
        "contract", "invoke",
        "--network", &network,
        "--source", source,
        "--id", id,
        "--",
        func,
    ];
    args.extend_from_slice(fn_args);
    let stdout = try_run(&args)?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(trimmed).map_err(|e| CliError {
        code: None,
        raw: format!("stdout not json: {e}\n{trimmed}"),
    })
}

/// Deploys `wasm_path` signed by `source`, forwarding `constructor_args` to
/// the contract's `__constructor`. Returns the new contract's C-strkey id.
pub fn deploy(source: &str, wasm_path: &str, constructor_args: &[&str]) -> String {
    let network =
        std::env::var("STELLAR_NETWORK").expect("STELLAR_NETWORK not set");
    let mut args: Vec<&str> = vec![
        "contract", "deploy",
        "--source", source,
        "--network", &network,
        "--wasm", wasm_path,
        "--",
    ];
    args.extend_from_slice(constructor_args);
    let stdout = run(&args);
    stdout
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with('C') && line.len() == 56)
        .unwrap_or_else(|| panic!("no contract id in deploy output:\n{stdout}"))
        .to_string()
}

/// POSTs `body` as JSON to the Soroban RPC at `rpc_url` and returns the
/// parsed response. Panics if curl fails or the body is not JSON.
pub fn rpc_call(rpc_url: &str, body: &str) -> Value {
    let out = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            rpc_url,
            "-H",
            "Content-Type: application/json",
            "-d",
            body,
        ])
        .output()
        .expect("failed to spawn curl");
    if !out.status.success() {
        panic!("curl failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).expect("RPC response not JSON")
}

/// Returns the latest ledger sequence reported by Soroban RPC at `rpc_url`.
pub fn rpc_latest_ledger(rpc_url: &str) -> u32 {
    let body =
        r#"{"jsonrpc":"2.0","id":1,"method":"getLatestLedger","params":{}}"#;
    let resp = rpc_call(rpc_url, body);
    resp["result"]["sequence"]
        .as_u64()
        .expect("ledger sequence missing") as u32
}

fn extract_error_code(s: &str) -> Option<u32> {
    let needle = "Error(Contract, #";
    let i = s.find(needle)?;
    let rest = &s[i + needle.len()..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}
