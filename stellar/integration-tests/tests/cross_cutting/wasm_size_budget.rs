//! Asserts the optimized contract WASMs ship under their size budgets.

use integration_tests::TestContext;

const MANAGER_BUDGET_BYTES: u64 = 90_000;
const TRANSCEIVER_BUDGET_BYTES: u64 = 35_000;
const MOCK_TOKEN_BUDGET_BYTES: u64 = 6_000;

fn wasm_size(path: &str) -> u64 {
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("cannot stat {path}: {e}"))
        .len()
}

/// Catches: accidental WASM bloat from a dependency bump or refactor pushing
/// any contract above its deploy-cost / instance-storage budget. A bloated
/// manager.wasm increases per-deployment fees and risks ledger entry limits.
#[test]
#[ignore]
fn contract_wasms_under_size_budget() {
    let ctx = TestContext::from_env();

    let mgr = wasm_size(&ctx.manager_wasm_path);
    let tcv = wasm_size(&ctx.transceiver_wasm_path);
    let mt = wasm_size(&ctx.mock_token_wasm_path);

    assert!(
        mgr <= MANAGER_BUDGET_BYTES,
        "manager.wasm {mgr} bytes exceeds budget {MANAGER_BUDGET_BYTES}"
    );
    assert!(
        tcv <= TRANSCEIVER_BUDGET_BYTES,
        "transceiver.wasm {tcv} bytes exceeds budget {TRANSCEIVER_BUDGET_BYTES}"
    );
    assert!(
        mt <= MOCK_TOKEN_BUDGET_BYTES,
        "mock_token.wasm {mt} bytes exceeds budget {MOCK_TOKEN_BUDGET_BYTES}"
    );
}
