# NTT Manager Contract

The NTT (Native Token Transfers) Manager is a Soroban smart contract that enables secure cross-chain token transfers between Stellar and other Wormhole-supported blockchains. It serves as the central orchestrator for token custody, message sequencing, and multi-transceiver attestation.

## Table of Contents

- [NTT Manager Contract](#ntt-manager-contract)
  - [Table of Contents](#table-of-contents)
  - [Overview](#overview)
  - [Architecture](#architecture)
  - [Key Concepts](#key-concepts)
    - [Operating Modes](#operating-modes)
    - [Transceivers](#transceivers)
    - [Peers](#peers)
    - [Rate Limiting](#rate-limiting)
    - [Attestation Threshold](#attestation-threshold)
  - [Transfer Flows](#transfer-flows)
    - [Outbound Transfer](#outbound-transfer)
    - [Inbound Transfer](#inbound-transfer)
  - [Module Reference](#module-reference)
  - [Storage Model](#storage-model)
    - [Instance Storage (loaded every invocation)](#instance-storage-loaded-every-invocation)
    - [Persistent Storage (per-entity data)](#persistent-storage-per-entity-data)
    - [TTL Configuration](#ttl-configuration)
  - [Error Codes](#error-codes)
  - [Security Considerations](#security-considerations)
    - [Reentrancy Protection](#reentrancy-protection)
    - [Replay Protection](#replay-protection)
    - [Authorization Model](#authorization-model)
  - [Integration Guide](#integration-guide)
    - [Deploying the Contract](#deploying-the-contract)
    - [Setting Up Transceivers](#setting-up-transceivers)
    - [Configuring Peers](#configuring-peers)
    - [Initiating Transfers](#initiating-transfers)
    - [Processing Inbound Transfers](#processing-inbound-transfers)
    - [Queue Management](#queue-management)
    - [Query Functions](#query-functions)

---

## Overview

The NTT Manager handles the complete lifecycle of cross-chain token transfers:

- **Outbound**: Takes custody of tokens (lock or burn), creates NTT messages, and dispatches them to transceivers for cross-chain delivery
- **Inbound**: Collects attestations from transceivers, verifies threshold requirements, and releases tokens to recipients (unlock or mint)
- **Rate Limiting**: Implements token bucket rate limiting for both outbound and per-chain inbound transfers
- **Queue Management**: Supports queueing transfers that exceed rate limits for later completion

---

## Address Resolution

Wormhole carries every address as 32 raw bytes, but a Soroban `Address` is either
a `G…` account or a `C…` contract — the raw bytes alone don't say which. The
manager therefore identifies Stellar addresses by `hash_address = keccak256(StrKey)`
and resolves the inbound recipient through a shared registry on the **Wormhole
core** contract, not locally.

- **Register once (per recipient).** Before receiving, a recipient calls
  `record_address` on the Wormhole core, which stores `hash_address(addr) → addr`.
  It is permissionless and idempotent.
- **Outbound.** `sender`, `source_token`, and the advertised manager id are
  encoded with `hash_address` — one canonical, collision-free identity per address.
- **Inbound.** The manager resolves the message's `to` (a `hash_address`) back to
  the real `Address` via `get_address_from_hash` on the core. An unregistered
  recipient fails with `RecipientNotRegistered` (66) **before** the message is
  marked executed, so the transfer reverts and is safely retryable once the
  recipient registers — no funds are lost.

The manager learns the core address at construction
(`__constructor(…, wormhole_core)`), exposed via `get_wormhole_core`.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          NTT Manager                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐   │
│  │   Outbound   │    │   Inbound    │    │    Transceivers      │   │
│  │   Module     │    │   Module     │    │    Registry          │   │
│  │              │    │              │    │                      │   │
│  │ • transfer() │    │ • attest()   │    │ • set_transceiver()  │   │
│  │ • queue()    │    │ • execute()  │    │ • remove_transceiver │   │
│  │ • cancel()   │    │ • complete() │    │ • set_threshold()    │   │
│  └──────┬───────┘    └──────┬───────┘    └──────────────────────┘   │
│         │                   │                                       │
│         ▼                   ▼                                       │
│  ┌──────────────────────────────────────-┐                          │
│  │           Token Operations            │                          │
│  │                                       │                          │
│  │  LOCKING MODE    │   BURNING MODE     │                          │
│  │  • lock()        │   • burn()         │                          │
│  │  • unlock()      │   • mint()         │                          │
│  └───────────────────────────────────────┘                          │
│                                                                     │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐   │
│  │ Rate Limiter │    │    Peers     │    │      Messages        │   │
│  │              │    │   Registry   │    │                      │   │
│  │ • outbound   │    │              │    │ • NttManagerMessage  │   │
│  │ • inbound    │    │ • per-chain  │    │ • NativeTokenTransfer│   │
│  │ • backflow   │    │ • decimals   │    │ • TrimmedAmount      │   │
│  └──────────────┘    └──────────────┘    └──────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
         │                                              │
         ▼                                              ▼
┌─────────────────┐                          ┌─────────────────┐
│  Transceiver 1  │         ...              │  Transceiver N  │
│  (Wormhole)     │                          │  (Other)        │
└─────────────────┘                          └─────────────────┘
         │                                              │
         └──────────────────┬───────────────────────────┘
                            ▼
                   Cross-Chain Messages
                   (via Wormhole VAAs)
```

---

## Key Concepts

### Operating Modes

The NTT Manager operates in one of two modes, determined at deployment:

| Mode | Outbound Action | Inbound Action | Use Case |
|------|-----------------|----------------|----------|
| **Locking** | Lock tokens in contract | Unlock tokens from contract | Canonical chain (holds real tokens) |
| **Burning** | Burn tokens | Mint tokens | Non-canonical chains (wrapped/synthetic tokens) |

```rust
pub enum Mode {
    Locking = 0,  // Token custody via lock/unlock
    Burning = 1,  // Token supply via burn/mint
}
```

**Important**: Burning mode requires a custom token contract with `burn(from, amount)` and `mint(to, amount)` functions that authorize the NTT Manager.

### Transceivers

Transceivers are external contracts responsible for cross-chain message delivery. The NTT Manager supports up to **64 transceivers** tracked via a bitmap.

```rust
pub struct TransceiverInfo {
    pub address: Address,   // Contract address
    pub enabled: bool,      // Currently active for attestations
    pub index: u32,         // Permanent index (0-63), never reused
}
```

Key properties:
- Indices are **permanent** - disabling a transceiver doesn't free its index
- The **enabled bitmap** (`u64`) tracks which transceivers are active
- The first transceiver registration automatically sets threshold to 1

### Peers

Peers represent NTT Managers on other chains. Each peer maintains:

```rust
pub struct NttManagerPeer {
    pub address: BytesN<32>,           // 32-byte address on remote chain
    pub token_decimals: u32,           // Token decimals on that chain (1-18)
    pub inbound_rate_limit: RateLimitParams,  // Per-chain rate limit
}
```

Peer validation ensures:
- Chain ID is not zero
- Chain ID is different from this contract's chain
- Address is not all zeros
- Decimals are between 1 and 18

### Rate Limiting

The contract implements a **token bucket** rate limiter for both directions:

```rust
pub struct RateLimitParams {
    pub limit: u64,              // Maximum bucket capacity
    pub current_capacity: u64,   // Current available capacity
    pub last_tx_timestamp: u64,  // Last update timestamp
}
```

**How it works**:
1. Capacity starts at `limit` (full bucket)
2. Each transfer consumes capacity equal to the trimmed amount
3. Capacity refills linearly over `RATE_LIMIT_DURATION` (default: 24 hours)
4. If capacity is insufficient, transfers can be queued (if `should_queue=true`)

**Backflow mechanism**: Inbound transfers refill outbound capacity and vice versa, maintaining bidirectional balance.

### Attestation Threshold

For inbound transfers to execute, they must receive attestations from at least `threshold` enabled transceivers:

```
attestation_count = popcount(attested_bitmap & enabled_bitmap)
transfer_approved = attestation_count >= threshold
```

**Invariants enforced**:
- INV-023: `threshold <= enabled_transceiver_count`
- INV-024: `threshold > 0` when transceivers exist

---

## Transfer Flows

### Outbound Transfer

```
User                    NTT Manager                Transceiver(s)
  │                          │                          │
  │  transfer(amount, ...)   │                          │
  │─────────────────────────>│                          │
  │                          │                          │
  │                    ┌─────┴─────-┐                   │
  │                    │ Validate:  │                   │
  │                    │ • amount>0 │                   │
  │                    │ • recipient│                   │
  │                    │ • peer     │                   │
  │                    └─────┬─────-┘                   │
  │                          │                          │
  │                    ┌─────┴─────┐                    │
  │                    │ Trim amt  │                    │
  │                    │ to 8 dec  │                    │
  │                    └─────┬─────┘                    │
  │                          │                          │
  │                    ┌─────┴─────┐                    │
  │                    │ Custody   │                    │
  │                    │ tokens    │                    │
  │                    │(lock/burn)│                    │
  │                    └─────┬─────┘                    │
  │                          │                          │
  │                    ┌─────┴─────┐                    │
  │                    │ Check rate│                    │
  │                    │  limit    │                    │
  │                    └─────┬─────┘                    │
  │                          │                          │
  │              ┌───────────┴───────────┐              │
  │              │                       │              │
  │         [Consumed]              [Delayed]           │
  │              │                       │              │
  │              ▼                       ▼              │
  │       ┌──────────┐           ┌──────────┐           │
  │       │ Send to  │           │ Queue    │           │
  │       │ all      │           │ transfer │           │
  │       │ enabled  │           │ for later│           │
  │       │ xceivers │           └──────────┘           │
  │       └────┬─────┘                                  │
  │            │          send_message()                │
  │            │─────────────────────────────────────>  │
  │            │                                        │
  │<───────────┴─────────────────────────────────────── │
  │     TransferResult{sequence, queued, digest}        │
```

### Inbound Transfer

```
Transceiver         NTT Manager                  Token Contract
     │                   │                             │
     │ attestation_      │                             │
     │ received(payload) │                             │
     │──────────────────>│                             │
     │                   │                             │
     │             ┌─────┴─────┐                       │
     │             │ Verify:   │                       │
     │             │ • xceiver │                       │
     │             │ • peer    │                       │
     │             │ • not dup │                       │
     │             └─────┬─────┘                       │
     │                   │                             │
     │             ┌─────┴─────┐                       │
     │             │ Record    │                       │
     │             │ attestation│                      │
     │             │ in bitmap │                       │
     │             └─────┬─────┘                       │
     │                   │                             │
     │             ┌─────┴─────┐                       │
     │             │ Check     │                       │
     │             │ threshold │                       │
     │             └─────┬─────┘                       │
     │                   │                             │
     │       ┌───────────┴───────────┐                 │
     │       │                       │                 │
     │  [Below threshold]    [Threshold met]           │
     │       │                       │                 │
     │       ▼                       ▼                 │
     │  Return early         ┌──────────┐             │
     │  (wait for more)      │ Check    │             │
     │                       │ inbound  │             │
     │                       │ rate lim │             │
     │                       └────┬─────┘             │
     │                            │                    │
     │              ┌─────────────┴─────────────┐     │
     │              │                           │     │
     │         [Consumed]                  [Delayed]  │
     │              │                           │     │
     │              ▼                           ▼     │
     │       ┌──────────┐              ┌──────────┐   │
     │       │ Release  │              │ Queue    │   │
     │       │ tokens   │              │ transfer │   │
     │       │──────────────────────────────────>│   │
     │       └──────────┘              └──────────┘   │
     │                                                 │
     │<────────────────────────────────────────────────│
     │     AttestationResult{approved, executed, queued}
```

---

## Module Reference

| Module | File | Purpose |
|--------|------|---------|
| **lib.rs** | [lib.rs](src/lib.rs) | Contract entry points and public API |
| **state.rs** | [state.rs](src/state.rs) | Core types (`Mode`, `DataKey`, queue structs) |
| **storage.rs** | [storage.rs](src/storage.rs) | Type-safe storage wrappers with TTL extension |
| **constants.rs** | [constants.rs](src/constants.rs) | TTL values, rate limit duration, max transceivers |
| **errors.rs** | [errors.rs](src/errors.rs) | Error enum with categorized codes |
| **messages.rs** | [messages.rs](src/messages.rs) | `TrimmedAmount`, `NativeTokenTransfer`, `NttManagerMessage` |
| **rate_limit.rs** | [rate_limit.rs](src/rate_limit.rs) | Token bucket rate limiter implementation |
| **token_ops.rs** | [token_ops.rs](src/token_ops.rs) | Lock/unlock/burn/mint operations |
| **peers.rs** | [peers.rs](src/peers.rs) | Peer registry and inbound rate limiting |
| **transceivers.rs** | [transceivers.rs](src/transceivers.rs) | Transceiver registry and bitmap operations |
| **outbound.rs** | [outbound.rs](src/outbound.rs) | Outbound transfer logic and queue management |
| **inbound.rs** | [inbound.rs](src/inbound.rs) | Attestation processing and inbound execution |

---

## Storage Model

### Instance Storage (loaded every invocation)

| Key | Type | Description |
|-----|------|-------------|
| `Admin` | `Address` | Contract administrator |
| `PendingAdmin` | `Address` | Pending admin for 2-step transfer |
| `Pauser` | `Address` | Optional pause-capable address |
| `Token` | `Address` | Managed token contract |
| `TokenDecimals` | `u32` | Cached token decimals |
| `Mode` | `Mode` | Locking or Burning |
| `ChainId` | `u32` | This chain's Wormhole chain ID |
| `Paused` | `bool` | Contract pause state |
| `Threshold` | `u32` | Required attestation count |
| `NextSequence` | `u64` | Next outbound sequence number |
| `Version` | `u32` | Contract version |
| `TransceiverCount` | `u32` | Total registered transceivers |
| `EnabledBitmap` | `u64` | Bitmap of enabled transceivers |
| `OutboundRateLimit` | `RateLimitParams` | Global outbound rate limit |
| `RateLimitDuration` | `u64` | Rate limit refill period (seconds) |

### Persistent Storage (per-entity data)

| Key | Type | Description |
|-----|------|-------------|
| `Peer(chain_id)` | `NttManagerPeer` | Peer config by chain ID |
| `Transceiver(index)` | `TransceiverInfo` | Transceiver by index |
| `TransceiverIndex(address)` | `u32` | Reverse lookup: address → index |
| `Attestation(digest)` | `AttestationInfo` | Attestation state by message digest |
| `OutboundQueue(sequence)` | `OutboundQueuedTransfer` | Queued outbound transfers |
| `InboundQueue(digest)` | `InboundQueuedTransfer` | Queued inbound transfers |

### TTL Configuration

Defined in [`soroban_ntt_client::constants`](../soroban-ntt-client/src/constants.rs)
and shared with every contract in the workspace:

```rust
pub const TTL_THRESHOLD: u32 = 17280;        // ~1 day
pub const TTL_EXTEND:    u32 = 17280 * 30;   // ~30 days
```

---

## Error Codes

| Range | Category | Examples |
|-------|----------|----------|
| 1-9 | Message parsing | `MessageTooShort`, `InvalidPrefix`, `InvalidDecimals` |
| 10-19 | Authorization | `Unauthorized`, `InvalidPendingAdmin`, `ContractPaused` |
| 20-29 | Rate limiting | `RateLimitNotInitialized` |
| 30-39 | Initialization | `NotInitialized` |
| 40-49 | Transceivers | `TransceiverNotRegistered`, `MaxTransceiversReached`, `ZeroThreshold` |
| 50-59 | Peers | `PeerNotFound`, `InvalidPeerChainIdZero`, `InvalidPeer` |
| 60-69 | Transfers | `ZeroAmount`, `InvalidRecipient`, `TransferExceedsRateLimit`, `RecipientNotRegistered` |
| 70-79 | Reentrancy | `Reentering` |
| 80-89 | Attestation | `TransceiverNotEnabled`, `TransferAlreadyRedeemed` |

---

## Security Considerations

### Reentrancy Protection

All state-modifying operations use `with_transfer_guard()` which:
1. Checks contract is not paused
2. Sets a reentrancy flag in temporary storage
3. Clears the flag after execution (even on error)

### Replay Protection

- Each message has a unique digest computed from `source_chain + message_content`
- Attestations are tracked per-transceiver per-digest
- Executed transfers are marked to prevent double redemption

### Authorization Model

| Operation | Required Auth |
|-----------|---------------|
| Transfer tokens | Sender (via `require_auth`) |
| Admin functions | Admin address |
| Pause/Unpause | Admin or Pauser |
| Cancel queued transfer | Original sender |
| Complete queued transfer | Permissionless |
| Attest message | Transceiver contract |


## Integration Guide

### Deploying the Contract

```rust
// Constructor parameters
__constructor(
    env: Env,
    admin: Address,           // Contract administrator
    token: Address,           // Token to manage
    mode: Mode,               // Locking or Burning
    chain_id: u32,            // Wormhole chain ID (61 for Stellar)
    outbound_limit: u64,      // Max outbound per rate limit period
    rate_limit_duration: u64, // Period in seconds (e.g., 86400 for 24h)
)
```

### Setting Up Transceivers

```rust
// Register a transceiver (admin only)
set_transceiver(env, admin, transceiver_address) -> Result<u32, Error>

// Set attestation threshold
set_threshold(env, admin, threshold) -> Result<(), Error>
```

### Configuring Peers

```rust
// Register a peer NTT Manager on another chain
set_peer(
    env,
    admin,
    chain_id: u32,           // Remote Wormhole chain ID
    peer_address: BytesN<32>, // Remote manager address
    token_decimals: u32,      // Token decimals on remote chain
    inbound_limit: u64,       // Inbound rate limit for this chain
) -> Result<(), Error>
```

### Initiating Transfers

```rust
// Basic transfer
transfer(
    env,
    sender: Address,
    amount: i128,            // In local token decimals
    recipient_chain: u32,    // Destination chain ID
    recipient: BytesN<32>,   // Recipient address (32 bytes)
    should_queue: bool,      // Queue if rate limited?
) -> Result<TransferResult, Error>

// Transfer with custom payload
transfer_with_payload(
    env,
    sender,
    amount,
    recipient_chain,
    recipient,
    should_queue,
    additional_payload: Bytes,
) -> Result<TransferResult, Error>
```

### Processing Inbound Transfers

Transceivers call:
```rust
attestation_received(
    env,
    transceiver: Address,       // Must be registered & enabled
    source_chain: u32,
    source_ntt_manager: BytesN<32>,
    payload: Bytes,             // Encoded NttManagerMessage
) -> Result<AttestationResult, Error>
```

### Queue Management

```rust
// Complete a queued outbound transfer (permissionless)
complete_queued_transfer(env, sequence) -> Result<TransferResult, Error>

// Cancel and refund a queued transfer (sender only)
cancel_queued_transfer(env, sender, sequence) -> Result<(), Error>

// Complete a queued inbound transfer (permissionless)
complete_inbound_transfer(env, digest) -> Result<(), Error>
```

### Query Functions

```rust
get_token(env) -> Address
get_mode(env) -> Mode
get_chain_id(env) -> u32
get_threshold(env) -> u32
get_peer(env, chain_id) -> Option<NttManagerPeer>
get_outbound_capacity(env) -> u64
get_inbound_limit_params(env, chain_id) -> Option<RateLimitParams>
is_message_executed(env, digest) -> bool
quote_transfer(env, amount, recipient_chain) -> (trimmed_amount, dust)
```

---
