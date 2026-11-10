# Vendored Wormhole core WASM

`wormhole_core.wasm` is the Stellar Soroban port of the Wormhole core, built
once locally and committed here so the IT harness has a reproducible artifact.

## Provenance

- Source repo: <https://github.com/NethermindEth/wormhole>
- Branch: `ext-integration/stellar`
- Commit: `946cef0d25f2b49de37d1ce5192248409dd44a2e` (the same commit
  `stellar/Cargo.lock` pins for the `wormhole-soroban-client` crate)
- Crate: `stellar/contracts/wormhole-contract`
- Build command (run inside the cloned repo's `stellar/` directory):

  ```sh
  stellar contract build --package wormhole-contract --optimize --locked
  ```

- Output copied: `target/wasm32v1-none/release/wormhole_contract.wasm`
  → `stellar/integration-tests/vendor/wormhole_core.wasm`
- Size: 24746 bytes
- SHA-256: `6d9dca444dc33fda290ea34d4f3e96fb4a7349be37e9039f988dd0b27b103cb6`
- Toolchain: `stellar 28.0.0`, `rustc 1.89.0` (from the repo's `rust-toolchain.toml`)

## Test guardian set

The IT harness initialises this core with a one-guardian test set. The
secp256k1 secret is in `stellar/integration-tests/.env.localnet` as
`GUARDIAN_SECRET_HEX`, derived from `keccak256("ntt-stellar-it-test-guardian-v1")`:

- Secret (hex): `71382df8ac8e4ad5035ccb62b18369d1f720811a67f4a13fd8fc9ce3734dbf77`
- Guardian eth address: `0xEEb882919aEF1cF6B745208874B37fE9fEE74001`

**LOCALNET-ONLY KEY. DO NOT REUSE ON TESTNET OR MAINNET.**

## Refreshing this WASM

Re-clone the source repo at the commit `stellar/Cargo.lock` pins for
`wormhole-soroban-client`, re-run the build, copy the WASM here, and update the
commit hash, size, SHA-256 and toolchain above.
