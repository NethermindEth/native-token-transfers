# soroban-ntt-client

The shared library that the [manager](../manager) and [transceiver](../transceiver) contracts both build on. It is not a deployable contract (it declares no `#[contract]` and builds as a plain `lib`). It is the single definition of everything the two contracts must agree on, both with each other and with NTT deployments on other chains:

- the **cross-chain wire formats** (byte layouts and magic prefixes),
- the **shared data types** exchanged across the two contracts,
- the **error vocabularies**, **events**, and **protocol constants**, and
- the **cross-contract interfaces** used to call between contracts.

`#![no_std]`. Depends only on `soroban-sdk` and [`wormhole-soroban-client`](https://github.com/NethermindEth/wormhole) (for the `BytesReader` decoder and Wormhole address hashing). Imported everywhere as `soroban_ntt_client::…`.

## Why the ABI lives in one crate

Two facts make this crate the workspace's breaking-change surface, so any edit here should be treated as an ABI change:

1. **Interface traits are used on both sides of every cross-contract call:** each interface (`NttManagerInterface`, `TransceiverInterface`, `WormholeTransceiverInterface`, `RateLimiterInterface`) carries `#[contractclient]`. The contract *implements* the trait (server side); the macro *generates* a `…Client` struct used to call it (client side). The manager calls the transceiver through `TransceiverClient`; the transceiver calls back through `NttManagerClient`. One signature change moves both sides at once.
2. **Message codecs and error discriminants are on-chain ABI:** field order in a `#[contracttype]`, the byte layout of a message, the magic prefixes, and the numeric value of each `#[contracterror]` variant are all observed off-chain and by peer chains. Appending is safe. Reordering or renumbering is not.

## Module map

| Module | Contents |
|--------|----------|
| [`constants`](src/constants.rs) | Storage TTLs, registry cap, rate-limit window, wire prefixes, transceiver type id |
| [`errors`](src/errors.rs) | `NttManagerError` and `TransceiverError` enums, plus `From<WormholeError>` bridges |
| [`events`](src/events.rs) | 15 `#[contractevent]` structs and their `emit_*` helpers |
| [`types`](src/types.rs) | Shared state and return types (`Mode`, peers, queued transfers, results) |
| [`rate_limit`](src/rate_limit.rs) | `RateLimitParams` token bucket and the `RateLimiterInterface` |
| [`manager`](src/manager.rs) | `NttManagerInterface` and its generated `NttManagerClient` |
| [`transceiver`](src/transceiver.rs) | Transport-agnostic `TransceiverInterface` and `TransceiverClient` |
| [`wormhole_transceiver`](src/wormhole_transceiver.rs) | Wormhole-specific `WormholeTransceiverInterface` and client |
| [`messages`](src/messages/) | Wire codecs: `ntt_message`, `transceiver_message`, `trimmed_amount` |
| [`utils`](src/utils.rs) | `hash_address` helpers, chain-id narrowing, `flatten_call` |

## Cross-chain message formats

An NTT token transfer is three payloads nested inside each other, then carried as the payload of a Wormhole VAA. The manager owns the inner two layers; the transceiver owns the envelope; Wormhole owns the VAA.

```
Wormhole VAA
└── payload = TransceiverMessage        prefix 0x9945FF10
    ├── source_manager   (32 bytes)
    ├── recipient_manager(32 bytes)
    ├── manager_payload = NttManagerMessage
    │   ├── id     (32 bytes)
    │   ├── sender (32 bytes)
    │   └── payload = NativeTokenTransfer   prefix 0x994E5454
    │       ├── TrimmedAmount (decimals + amount)
    │       ├── source_token (32 bytes)
    │       ├── to           (32 bytes)
    │       ├── to_chain     (u16)
    │       └── additional_payload (optional)
    └── transceiver_payload (reserved, empty here)
```

All multi-byte integers are big-endian. Decoders use `wormhole_soroban_client::BytesReader`; a truncated read returns `MessageTooShort`. The prefixes match the canonical NTT values used by the EVM, Solana, and Sui implementations, which is what lets a Stellar transfer land on any of them and back.

### TrimmedAmount (9 bytes)

Defined in [`trimmed_amount.rs`](src/messages/trimmed_amount.rs). Not a standalone framed message; it is decoded inline inside `NativeTokenTransfer`.

| Offset | Size | Field | Notes |
|-------:|-----:|-------|-------|
| 0 | 1 | `decimals` | struct stores `u32`, written as `u8` |
| 1 | 8 | `amount` | `u64`, big-endian |

### NativeTokenTransfer (79+ bytes)

The transfer itself. `MIN_SIZE = 79`. Defined in [`ntt_message.rs`](src/messages/ntt_message.rs).

| Offset | Size | Field | Notes |
|-------:|-----:|-------|-------|
| 0 | 4 | prefix | `NTT_PREFIX` `0x994E5454`; mismatch is `InvalidPrefix` |
| 4 | 1 | `decimals` | inline `TrimmedAmount` |
| 5 | 8 | `amount` | inline `TrimmedAmount` |
| 13 | 32 | `source_token` | source-chain token, 32-byte form |
| 45 | 32 | `to` | recipient, 32-byte form |
| 77 | 2 | `to_chain` | Wormhole chain id (`u16`); struct field is `u32` |
| 79 | 2 | `additional_payload_len` | present only if bytes remain |
| 81 | var | `additional_payload` | up to 65535 bytes, else `PayloadTooLong` |

The optional payload is detected purely by "are there bytes left". That is safe because the outer `NttManagerMessage` frames this payload with an explicit length, so the slice handed to the decoder is exactly the transfer.

### NttManagerMessage (66+ bytes)

Wraps a transfer with a unique id and the original sender. `MIN_SIZE = 66`.

| Offset | Size | Field | Notes |
|-------:|-----:|-------|-------|
| 0 | 32 | `id` | usually `sequence_to_message_id(sequence)` |
| 32 | 32 | `sender` | original sender, 32-byte form |
| 64 | 2 | `payload_len` | `u16`, big-endian |
| 66 | var | `payload` | an encoded `NativeTokenTransfer` |

### TransceiverMessage (72+ bytes)

The envelope a transceiver posts to Wormhole. `MIN_SIZE = 72`. Defined in [`transceiver_message.rs`](src/messages/transceiver_message.rs).

| Offset | Size | Field | Notes |
|-------:|-----:|-------|-------|
| 0 | 4 | prefix | `WH_TRANSCEIVER_PREFIX` `0x9945FF10`; mismatch is `InvalidTransceiverPrefix` |
| 4 | 32 | `source_manager` | manager that produced the payload |
| 36 | 32 | `recipient_manager` | manager expected to consume it |
| 68 | 2 | `manager_payload_len` | `u16`, big-endian |
| 70 | var | `manager_payload` | an encoded `NttManagerMessage` |
| + | 2 | `transceiver_payload_len` | `u16`, big-endian |
| + | var | `transceiver_payload` | reserved (see below) |

`transceiver_payload` is reserved by the cross-chain protocol for transceiver-private metadata. The Wormhole transceiver never fills it, but the field is still encoded and decoded so the bytes stay compatible with peer chains that do use it.

### Accountant broadcasts

Two one-way payloads announce a transceiver's configuration to the Wormhole [Global Accountant](https://wormhole.com/docs/products/token-transfers/native-token-transfers/). They are encode-only (no decoder, not `#[contracttype]`) and posted by the permissionless `broadcast_id` / `broadcast_peer` entry points on the transceiver.

`WormholeTransceiverInfo` (70 bytes): prefix `0x9C23BD3B`, then `manager_address(32)`, `manager_mode(1)`, `token_address(32)`, `token_decimals(1)`.

`WormholeTransceiverRegistration` (38 bytes): prefix `0x18FC67C2`, then `chain_id(2, u16)`, `transceiver_address(32)`.

### Message digest

The manager tracks attestations and dedup by digest:

```
digest = keccak256( source_chain (u16, big-endian) || NttManagerMessage bytes )
```

The two-byte source-chain prefix binds the digest to its origin, so the same message from two chains yields two different digests. This matches the EVM formula.

## Decimal normalization

Different chains hold the same token at different decimal precisions, and the wire amount is a `u64`. NTT reconciles this by trimming every amount to a shared precision before it crosses a chain boundary.

- The target precision is `min(8, source_decimals, destination_decimals)`. The ceiling is `TrimmedAmount::MAX_DECIMALS = 8`; it is not always 8.
- `trim(amount, from, to)` returns `(TrimmedAmount, dust)`, where `dust` is the sub-precision remainder that cannot be represented. The manager leaves dust with the sender rather than destroying it, so accounting stays exact.
- `untrim(to_decimals)` expands a trimmed amount back to a local precision on the receiving side.

`checked_add` / `checked_sub` require matching decimals and error with `DecimalMismatch` otherwise. Note that the arithmetic itself saturates rather than erroring, so those methods guard decimals, not numeric overflow.

## Protocol constants

Defined in [`constants.rs`](src/constants.rs). Shared by every contract in the workspace.

| Constant | Value | Meaning |
|----------|-------|---------|
| `TTL_THRESHOLD` | `17280` | Storage TTL floor in ledgers (about 1 day at 5s/ledger) before extension |
| `TTL_EXTEND` | `17280 * 30` | TTL extension in ledgers (about 30 days) |
| `MAX_TRANSCEIVERS` | `64` | Registry cap, bounded by the `u64` enabled bitmap |
| `RATE_LIMIT_DURATION` | `86400` | Default rate-limit refill window in seconds (24h) |
| `NTT_PREFIX` | `0x994E5454` | `NativeTokenTransfer` magic bytes |
| `WH_TRANSCEIVER_PREFIX` | `0x9945FF10` | `TransceiverMessage` magic bytes |
| `BROADCAST_ID_PREFIX` | `0x9C23BD3B` | `WormholeTransceiverInfo` magic bytes |
| `BROADCAST_PEER_PREFIX` | `0x18FC67C2` | `WormholeTransceiverRegistration` magic bytes |
| `WORMHOLE_TRANSCEIVER_TYPE` | `b"wormhole"` | Transceiver-type id read by off-chain tooling |

## Error vocabularies

Two `#[repr(u32)]` `#[contracterror]` enums in [`errors.rs`](src/errors.rs). Variants are grouped by numeric range so callers can classify a failure without string matching.

`NttManagerError`:

| Range | Category |
|-------|----------|
| 1–8 | Message encoding, decimals, chain-id, amount overflow |
| 13 | `NotAdminOrPauser` |
| 20, 30 | Uninitialized rate limit or state |
| 40–49 | Transceiver registry, threshold, transceiver-call failures |
| 50–55 | Peer registration and validation |
| 60–67 | Outbound and inbound transfer flow (includes `RecipientNotRegistered = 66`, `WormholeCoreCallFailed = 67`) |
| 80–84 | Attestation and redemption |

`TransceiverError`:

| Range | Category |
|-------|----------|
| 1 | `NotInitialized` |
| 10–15 | Peer registration and chain-id validation (`PeerAlreadySet = 12` marks one-shot registration) |
| 20–23 | Wormhole core interactions and manager queries |
| 30–36 | Message decoding and attestation dispatch |

Owner and pause failures do not appear in these enums. They surface from the OpenZeppelin Stellar libraries as `OwnableError` (2100–2102), `RoleTransferError` (2200–2203), and `PausableError::EnforcedPause` (1000).

## Events

Event structs live here so any observer (indexers, the Global Accountant, the integration tests) reads them from one definition. Each has an `emit_*` helper the contracts call after a successful state change. The manager emits `TransferSent`, `TransferRedeemed`, `MessageAttestedTo`, `ThresholdChanged`, `TransceiverAdded`, `TransceiverRemoved`, `PeerUpdated`, `OutboundTransferCancelled`, `OutboundTransferQueued`, `OutboundTransferRateLimited`, `InboundTransferQueued`, and `MessageAlreadyExecuted`. The transceiver emits `MessageSent`, `MessageReceived`, and `PeerSet`. See [`events.rs`](src/events.rs) for the exact topics and fields.

## Cross-contract interfaces

| Interface | Client | Role |
|-----------|--------|------|
| [`NttManagerInterface`](src/manager.rs) | `NttManagerClient` | Full manager API; implemented by the manager, called by the transceiver for `attestation_received` and `get_token` |
| [`TransceiverInterface`](src/transceiver.rs) | `TransceiverClient` | Transport-agnostic surface the manager relies on: `send_message`, `quote_delivery_price`, `get_manager_id` |
| [`WormholeTransceiverInterface`](src/wormhole_transceiver.rs) | `WormholeTransceiverClient` | Wormhole-specific peer management and the inbound VAA path |
| [`RateLimiterInterface`](src/rate_limit.rs) | `RateLimiterClient` | Read-only rate-limit and queue views |

## Notes for auditors

- **`checked_add` / `checked_sub` saturate:** they only error on decimal mismatch. Despite the name, numeric overflow silently clamps to `u64::MAX` or `0`.
- **All decoder failures collapse to `MessageTooShort`:** the `From<WormholeError>` bridge maps every `BytesReader` error to one variant, so a bad internal length prefix that over-reads is indistinguishable from a truncated buffer.
- **Chain ids are `u32` in structs but `u16` on the wire:** `validate_chain_id` narrows and rejects anything above `u16::MAX` with `ChainIdTooLarge`. Chain ids throughout are Wormhole chain ids (Stellar is 61), not native chain ids.
- **The attested bitmap is a `u64`:** this is why `MAX_TRANSCEIVERS` is 64. A transceiver's permanent index is its bit position.
