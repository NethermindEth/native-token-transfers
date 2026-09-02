# NTT with executor contract

`NttWithExecutor` is a thin Soroban wrapper that, in a single Stellar
transaction, initiates a Native Token Transfer through an NTT Manager **and**
registers a matching execution request with the Wormhole Executor so a relay
provider delivers the transfer on the destination chain.

## Table of contents

- [NTT with executor contract](#ntt-with-executor-contract)
  - [Overview](#overview)
  - [Why a contract](#why-a-contract)
  - [Transfer flow](#transfer-flow)
  - [Public API](#public-api)
    - [Constructor](#constructor)
    - [transfer](#transfer)
  - [Referrer fee](#referrer-fee)
  - [ERN1 request format](#ern1-request-format)
  - [Authorization model](#authorization-model)
  - [Error codes](#error-codes)
  - [Module reference](#module-reference)
  - [Executor binding](#executor-binding)
  - [Build and test](#build-and-test)

---

## Overview

Without this wrapper, a Stellar-originated NTT transfer has no standard way to be
auto-relayed: the transfer emits a Wormhole message, but nothing pays a provider
to deliver it. `NttWithExecutor` mirrors the EVM `NttManagerWithExecutor` and
Solana `example_ntt_with_executor` shims — it orchestrates both halves under the
sender's authorization and takes **no custody** of tokens.

A single deployment serves every manager on the chain: the executor is bound once
at construction, while the NTT manager is a per-call argument.

---

## Why a contract

A Soroban transaction permits only **one** top-level `InvokeHostFunction`
operation. Unlike Sui PTBs, `manager.transfer` and
`executor.request_execution` cannot be batched client-side, so this wrapper
composes the two calls on-chain instead.

---

## Transfer flow

```text
Sender              NttWithExecutor            Manager / Token / Executor
  │                        │                              │
  │  transfer(...)         │                              │
  │───────────────────────>│                              │
  │                  ┌──────┴──────┐                       │
  │                  │ require_auth │                      │
  │                  └──────┬──────┘                       │
  │                        │      get_peer(dst_chain)      │
  │                        │─────────────────────────────> │  → PeerNotFound if absent
  │                        │                               │
  │                  ┌──────┴───────┐                      │
  │                  │ referrer_fee │  (trimmed to dst)    │
  │                  └──────┬───────┘                      │
  │                        │   token.transfer(fee) [dbps>0]│
  │                        │─────────────────────────────> │  sender → referrer
  │                        │                               │
  │                        │ manager.transfer(amount-fee,  │
  │                        │        should_queue = false)  │
  │                        │─────────────────────────────> │  → sequence
  │                        │                               │
  │                        │ executor.request_execution(   │
  │                        │   dst=peer, payer=sender,     │
  │                        │   ERN1(sequence), quote, ...)  │
  │                        │─────────────────────────────> │  pulls XLM fee
  │<───────────────────────┤                               │
  │        sequence        │                               │
```

All three token movements — the optional referrer fee, the manager's lock/burn,
and the executor's XLM fee — settle under one authorization tree rooted at
`sender`.

---

## Public API

### Constructor

```rust
__constructor(env: Env, executor: Address)
```

Binds the Wormhole Executor address for the contract's lifetime.

### transfer

```rust
transfer(
    env: Env,
    sender: Address,
    ntt_manager: Address,
    amount: i128,
    destination: Destination,
    fee: FeeArgs,
    exec: ExecutorArgs,
) -> Result<u64, WrapperError>   // returns the NTT message sequence
```

```rust
pub struct Destination {
    pub chain: u32,           // Wormhole chain id of the destination
    pub recipient: BytesN<32>,
}

pub struct FeeArgs {
    pub referrer: Address,
    pub dbps: u32,            // tenths of a basis point; denominator 100_000
}

pub struct ExecutorArgs {
    pub payee: Address,       // executor pays this, bound to the signed quote
    pub amount: i128,         // XLM execution fee the executor pulls from sender
    pub refund: Address,      // destination-chain relay refund address
    pub signed_quote: Bytes,  // opaque EQ01 quote
    pub relay_instructions: Bytes,  // opaque
}
```

The wrapper does not expose `should_queue`; it always calls the manager with
`should_queue = false`. A rate-limited (queued) transfer emits no message yet, so
paying a relayer for it would be premature; the whole call reverts instead.

The Executor API produces `payee`, `signed_quote`, and `relay_instructions`
off-chain, and the executor validates them; this contract does not.

---

## Referrer fee

The wrapper takes the referrer fee from the transfer amount, in the transfer
token's own decimals, before bridging the remainder:

```text
raw = amount * dbps / 100_000
fee = trim(raw, src_decimals, dst_decimals)   // remove_dust, matching the bridge
```

Trimming reuses `TrimmedAmount::remove_dust` so the fee never carries precision
the bridge would silently drop. `dbps` must fit the `u16` on-wire fee field;
`dbps == 0` or `amount <= 0` yields no fee.

---

## ERN1 request format

The `request` bytes forwarded to the executor identify the NTT message to deliver:

```text
"ERN1"(4) || srcChain(u16 BE) || srcManager(32) || messageId(32)
```

- `srcChain` — the manager's chain id (61 for Stellar) narrowed to `u16`.
- `srcManager` — the manager contract's 32-byte identifier.
- `messageId` — the sequence right-aligned in 32 bytes.

Encoding delegates to `executor_requests::make_ntt_v1_request`.

---

## Authorization model

`sender.require_auth()` roots a single authorization tree. Each sub-invocation is
authorized under it:

| Sub-invocation                | Auth required         |
| ----------------------------- | --------------------- |
| Referrer fee `token.transfer` | `sender`              |
| `manager.transfer`            | `sender`              |
| `executor.request_execution`  | `sender` (as `payer`) |

The wrapper never holds tokens; the manager receives the real user as
`sender`, and the executor pulls the XLM fee directly from `sender`.

---

## Error codes

| Code | Variant              | Meaning                                                                     |
| ---- | -------------------- | --------------------------------------------------------------------------- |
| 1    | `InvalidReferrerFee` | `dbps` exceeds the `u16` fee field, or the fee does not fit the wire amount |
| 2    | `PeerNotFound`       | no registered peer for the recipient chain                                  |
| 3    | `FeeExceedsAmount`   | the referrer fee exceeds the amount                                         |

Manager and executor failures trap through their typed clients and revert the
whole transaction with the underlying contract's own error code.

---

## Module reference

| Module          | File                           | Purpose                                                                   |
| --------------- | ------------------------------ | ------------------------------------------------------------------------- |
| **lib.rs**      | [lib.rs](src/lib.rs)           | Contract entry points, `transfer` orchestration, argument and error types |
| **fee.rs**      | [fee.rs](src/fee.rs)           | Referrer fee calculation and dust trimming                                |
| **encoding.rs** | [encoding.rs](src/encoding.rs) | ERN1 request encoding                                                     |
| **executor.rs** | [executor.rs](src/executor.rs) | Typed client binding for the Executor                                     |
| **tests.rs**    | [tests.rs](src/tests.rs)       | Boundary tests with mock manager, executor, and token                     |

---

## Executor binding

[`executor.rs`](src/executor.rs) declares the Executor's `request_execution`
interface and generates its `ExecutorClient` locally. The upstream
`executor-soroban-client` targets a soroban-sdk release beyond the one this
workspace pins, so this crate regenerates the binding against the workspace SDK;
the resulting cross-contract call is identical on the wire.

---

## Build and test

```bash
cargo build                                   # from stellar/
cargo test  -p stellar-ntt-with-executor
cargo build -p stellar-ntt-with-executor --target wasm32v1-none --release
```
