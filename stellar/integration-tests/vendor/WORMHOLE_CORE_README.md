# Vendored Wormhole core WASM

`wormhole_core.wasm` is the Stellar Soroban port of the Wormhole core, built
once locally and committed here so the IT harness has a reproducible artifact.

## Provenance

- Source repo: <https://github.com/NethermindEth/wormhole>
- Branch: `stellar`
- Commit: `f379981979269c7baae15b3b8df2f4c50d61cd91`
- Crate: `stellar/contracts/wormhole-contract`
- Build command (run inside the cloned repo's `stellar/` directory):

  ```sh
  stellar contract build --package wormhole-contract
  stellar contract optimize --wasm target/wasm32v1-none/release/wormhole_contract.wasm
  ```

- Output copied: `target/wasm32v1-none/release/wormhole_contract.optimized.wasm`
  → `stellar/integration-tests/vendor/wormhole_core.wasm`
- Size: 28297 bytes
- Toolchain: `stellar 26.0.0`

## Test guardian set

The IT harness initialises this core with a one-guardian test set. The
secp256k1 secret is in `stellar/integration-tests/.env.localnet` as
`GUARDIAN_SECRET_HEX`, derived from `keccak256("ntt-stellar-it-test-guardian-v1")`:

- Secret (hex): `71382df8ac8e4ad5035ccb62b18369d1f720811a67f4a13fd8fc9ce3734dbf77`
- Guardian eth address: `0xEEb882919aEF1cF6B745208874B37fE9fEE74001`

**LOCALNET-ONLY KEY. DO NOT REUSE ON TESTNET OR MAINNET.**

## Refreshing this WASM

To bump the pin, re-clone the source repo, pull `stellar`, re-run the build,
copy the optimised WASM here, and update the commit hash + size + toolchain
above.
