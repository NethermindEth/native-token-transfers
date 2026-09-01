# Vendored Wormhole Executor WASM

`wormhole_executors.wasm` is the Wormhole Executor rail — the prepaid delivery
payment contract that `contracts/ntt-with-executor` calls `request_execution`
on. It is committed here for the same reason as the core: the TypeScript
integration suite needs a deployable artifact, and the source lives in another
repository on a moving branch.

## Provenance

- Source repo: <https://github.com/NethermindEth/example-messaging-executor>
- Branch: `stellar`
- Commit: `5319457430c45ffcfa165b6628f5ff45ffedf16a` (the same commit
  `stellar/Cargo.lock` pins for the `executor-requests` crate)
- Crate: `stellar/contracts/wormhole-executors`
- Build command (run inside the cloned repo's `stellar/` directory):

  ```sh
  stellar contract build --optimize
  ```

- Output copied: `target/wasm32v1-none/release/wormhole_executors.wasm`
  → `stellar/integration-tests/vendor/wormhole_executors.wasm`
- Size: 6836 bytes
- SHA-256: `6061f9ac5a2bb5b4bea2b86721922574e0bf4c7b677bee48aea5527382d68f07`
- Toolchain: `stellar 26.0.0`, `rustc 1.92.0`

That repository is its own cargo workspace on `soroban-sdk` 27 and edition 2024,
so it cannot be a member of this one — which is why the artifact is vendored
rather than built by `scripts/build-wasms.sh`.

## What it validates

The Executor parses only the 68-byte quote header on-chain (`srcChain`,
`dstChain`, `expiry`, and the 32-byte `payee` at `quote[24..56]`) and takes the
signature on trust, verifying it off-chain instead. A test can therefore build a
quote header by hand and drive `NttWithExecutor.transfer` end to end without a
quoter that supports chain 61.

## Refreshing this WASM

Re-clone the source repo at the commit `stellar/Cargo.lock` pins for
`executor-requests`, re-run the build, copy the WASM here, and update the commit
hash, size, SHA-256 and toolchain above.
