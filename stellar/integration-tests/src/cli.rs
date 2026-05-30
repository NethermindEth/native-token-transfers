use serde_json::Value;
use std::process::Command;

pub struct CliError {
    pub code: Option<u32>,
    pub raw: String,
}

pub fn run(args: &[&str]) -> String {
    try_run(args).unwrap_or_else(|e| panic!("stellar CLI failed:\n{}", e.raw))
}

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

pub fn invoke(source: &str, id: &str, func: &str, fn_args: &[&str]) -> Value {
    try_invoke(source, id, func, fn_args)
        .unwrap_or_else(|e| panic!("invoke {func} failed:\n{}", e.raw))
}

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

fn extract_error_code(s: &str) -> Option<u32> {
    let needle = "Error(Contract, #";
    let i = s.find(needle)?;
    let rest = &s[i + needle.len()..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}
