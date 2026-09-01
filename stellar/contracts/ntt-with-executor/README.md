# NTT With Executor Contract

`NttWithExecutor` is a thin Soroban wrapper that, in a single Stellar
transaction, initiates a Native Token Transfer through an NTT Manager **and**
registers a matching execution request with the Wormhole Executor so a relay
provider delivers the transfer on the destination chain.

## Table of Contents

- [NTT With Executor Contract](#ntt-with-executor-contract)
  - [Overview](#overview)
  - [Why a Contract](#why-a-contract)
  - [Transfer Flow](#transfer-flow)
  - [Public API](#public-api)
    - [Constructor](#constructor)
    - [transfer](#transfer)
  - [Referrer Fee](#referrer-fee)
  - [ERN1 Request Format](#ern1-request-format)
  - [Authorization Model](#authorization-model)
  - [Error Codes](#error-codes)
  - [Module Reference](#module-reference)
  - [Executor Binding](#executor-binding)
  - [Build & Test](#build--test)

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

## Why a Contract

A Soroban transaction permits only **one** top-level `InvokeHostFunction`
operation. Unlike Sui PTBs, `manager.transfer` and
`executor.request_execution` cannot be batched client-side, so the two calls are
composed on-chain by this wrapper instead.

---

## Transfer Flow

```
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

`should_queue` is not exposed — the wrapper always calls the manager with
`should_queue = false`. A rate-limited (queued) transfer emits no message yet, so
paying a relayer for it would be premature; the whole call reverts instead.

`payee`, `signed_quote`, and `relay_instructions` are produced off-chain by the
Executor API and validated by the executor, not by this contract.

---

## Referrer Fee

The referrer fee is taken from the transfer amount, in the transfer token's own
decimals, before the remainder is bridged:

```
raw = amount * dbps / 100_000
fee = trim(raw, src_decimals, dst_decimals)   // remove_dust, matching the bridge
```

Trimming reuses `TrimmedAmount::remove_dust` so the fee never carries precision
the bridge would silently drop. `dbps` must fit the `u16` on-wire fee field;
`dbps == 0` or `amount <= 0` yields no fee.

---

## ERN1 Request Format

The `request` bytes forwarded to the executor identify the NTT message to deliver:

```
"ERN1"(4) || srcChain(u16 BE) || srcManager(32) || messageId(32)
```

- `srcChain` — the manager's chain id (61 for Stellar) narrowed to `u16`.
- `srcManager` — the manager contract's 32-byte identifier.
- `messageId` — the sequence right-aligned in 32 bytes.

Encoding delegates to `executor_requests::make_ntt_v1_request`.

---

## Authorization Model

`sender.require_auth()` roots a single authorization tree. Each sub-invocation is
authorized under it:

| Sub-invocation                | Auth required         |
| ----------------------------- | --------------------- |
| Referrer fee `token.transfer` | `sender`              |
| `manager.transfer`            | `sender`              |
| `executor.request_execution`  | `sender` (as `payer`) |

The wrapper never holds tokens; the manager sees the real user as `sender`, and
the executor pulls the XLM fee directly from `sender`.

---

## Error Codes

| Code | Variant              | Meaning                                    |
| ---- | -------------------- | ------------------------------------------ |
| 1    | `InvalidReferrerFee` | `dbps` exceeds the `u16` on-wire fee field |
| 2    | `PeerNotFound`       | no registered peer for the recipient chain |

Manager and executor failures trap through their typed clients and revert the
whole transaction with the underlying contract's own error code.

---

## Module Reference

| Module          | File                           | Purpose                                                                   |
| --------------- | ------------------------------ | ------------------------------------------------------------------------- |
| **lib.rs**      | [lib.rs](src/lib.rs)           | Contract entry points, `transfer` orchestration, argument and error types |
| **fee.rs**      | [fee.rs](src/fee.rs)           | Referrer fee calculation and dust trimming                                |
| **encoding.rs** | [encoding.rs](src/encoding.rs) | ERN1 request encoding                                                     |
| **executor.rs** | [executor.rs](src/executor.rs) | Typed client binding for the Executor                                     |
| **tests.rs**    | [tests.rs](src/tests.rs)       | Boundary tests with mock manager, executor, and token                     |

---

## Executor Binding

[`executor.rs`](src/executor.rs) declares the Executor's `request_execution`
interface and generates its `ExecutorClient` locally. The upstream
`executor-soroban-client` targets a newer soroban-sdk than this workspace pins,
so the binding is regenerated against the workspace SDK; the resulting
cross-contract call is identical on the wire.

---

## Build & Test

```bash
cargo build                                   # from stellar/
cargo test  -p stellar-ntt-with-executor
cargo build -p stellar-ntt-with-executor --target wasm32-unknown-unknown --release
```
