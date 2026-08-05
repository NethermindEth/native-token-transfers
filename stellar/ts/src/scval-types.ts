/**
 * Decoders for the Soroban `#[contracttype]` structs the NTT contracts return
 * (`soroban_ntt_client::{types, rate_limit, messages}` and the manager's
 * `TransceiverInfo`). The analogue of `sui/ts/src/bcs-types.ts`.
 *
 * `StellarPlatform.simulateRead` already runs `scValToNative`, which turns a
 * struct into a plain object keyed by its Rust field names — `u32`/`bool` stay
 * primitives, `u64`/`i128` become `bigint`, `BytesN<N>` becomes a `Buffer`,
 * `Address` becomes a StrKey string, and `Option::None` becomes `null`. These
 * decoders pin those field names (the on-chain layout is field-ordered, so a
 * rename is a breaking change) and narrow the loose result to a typed value.
 */
import type {
  Ntt,
  TrimmedAmount,
} from "@wormhole-foundation/sdk-definitions-ntt";

const isStruct = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function struct(value: unknown, name: string): Record<string, unknown> {
  if (!isStruct(value))
    throw new Error(`Expected a ${name} struct, got ${typeof value}`);
  return value;
}

function field<T>(
  s: Record<string, unknown>,
  name: string,
  is: (v: unknown) => v is T
): T {
  const value = s[name];
  if (!is(value)) throw new Error(`Bad "${name}" field: ${String(value)}`);
  return value;
}

// `Option::None` decodes to `null`; a missing key means the wrong struct.
function optionalField<T>(
  s: Record<string, unknown>,
  name: string,
  is: (v: unknown) => v is T
): T | null {
  if (!(name in s)) throw new Error(`Missing "${name}" field`);
  return s[name] == null ? null : field(s, name, is);
}

const isNumber = (v: unknown): v is number => typeof v === "number";
const isBigint = (v: unknown): v is bigint => typeof v === "bigint";
const isBoolean = (v: unknown): v is boolean => typeof v === "boolean";
const isBytes = (v: unknown): v is Uint8Array => v instanceof Uint8Array;
const isString = (v: unknown): v is string => typeof v === "string";

/**
 * Scalar returns. A contract's return type is part of its ABI, so a mismatch
 * here is a wiring bug — fail loudly rather than coercing.
 */
export function asNumber(value: unknown): number {
  if (!isNumber(value)) throw new Error(`Expected a u32, got ${typeof value}`);
  return value;
}

export function asBigint(value: unknown): bigint {
  if (!isBigint(value))
    throw new Error(`Expected a u64/i128, got ${typeof value}`);
  return value;
}

export function asBoolean(value: unknown): boolean {
  if (!isBoolean(value))
    throw new Error(`Expected a bool, got ${typeof value}`);
  return value;
}

/** `Bytes`/`BytesN<N>` decode to a `Buffer`, which is a `Uint8Array`. */
export function asBytes(value: unknown): Uint8Array {
  if (!isBytes(value)) throw new Error(`Expected Bytes, got ${typeof value}`);
  return value;
}

/** An `Address` decodes to its StrKey text; `StellarAddress` validates it. */
export function asAddress(value: unknown): string {
  if (!isString(value))
    throw new Error(`Expected an Address, got ${typeof value}`);
  return value;
}

/** `Mode` is a `#[repr(u32)]` unit enum, so it decodes to its discriminant. */
export function decodeMode(value: unknown): Ntt.Mode {
  if (value === 0) return "locking";
  if (value === 1) return "burning";
  throw new Error(`Unexpected Mode discriminant: ${String(value)}`);
}

export function decodeTrimmedAmount(value: unknown): TrimmedAmount {
  const s = struct(value, "TrimmedAmount");
  return {
    amount: field(s, "amount", isBigint),
    decimals: field(s, "decimals", isNumber),
  };
}

export type RateLimitParams = {
  limit: bigint;
  currentCapacity: bigint;
  /** Seconds, not milliseconds — Soroban ledger time. */
  lastTxTimestamp: bigint;
};

export function decodeRateLimitParams(value: unknown): RateLimitParams {
  const s = struct(value, "RateLimitParams");
  return {
    limit: field(s, "limit", isBigint),
    currentCapacity: field(s, "current_capacity", isBigint),
    lastTxTimestamp: field(s, "last_tx_timestamp", isBigint),
  };
}

export type NttManagerPeer = {
  /** The peer manager's 32-byte wire address, not a StrKey. */
  address: Uint8Array;
  tokenDecimals: number;
  inboundRateLimit: RateLimitParams;
};

export function decodeNttManagerPeer(value: unknown): NttManagerPeer {
  const s = struct(value, "NttManagerPeer");
  return {
    address: field(s, "address", isBytes),
    tokenDecimals: field(s, "token_decimals", isNumber),
    inboundRateLimit: decodeRateLimitParams(s["inbound_rate_limit"]),
  };
}

export type TransferResult = {
  sequence: bigint;
  queued: boolean;
  digest: Uint8Array;
};

export function decodeTransferResult(value: unknown): TransferResult {
  const s = struct(value, "TransferResult");
  return {
    sequence: field(s, "sequence", isBigint),
    queued: field(s, "queued", isBoolean),
    digest: field(s, "digest", isBytes),
  };
}

export type InboundQueuedTransfer = {
  recipient: string;
  amount: bigint;
  trimmedAmount: bigint;
  /** Seconds since the epoch at which the transfer becomes releasable. */
  releaseTimestamp: bigint;
};

export function decodeInboundQueuedTransfer(
  value: unknown
): InboundQueuedTransfer {
  const s = struct(value, "InboundQueuedTransfer");
  return {
    recipient: field(s, "recipient", isString),
    amount: field(s, "amount", isBigint),
    trimmedAmount: field(s, "trimmed_amount", isBigint),
    releaseTimestamp: field(s, "release_timestamp", isBigint),
  };
}

export type OutboundQueuedTransfer = {
  sender: string;
  amount: TrimmedAmount;
  recipientChain: number;
  recipientNttManager: Uint8Array;
  recipient: Uint8Array;
  sourceToken: Uint8Array;
  /** Seconds since the epoch at which the transfer becomes releasable. */
  releaseTimestamp: bigint;
  additionalPayload: Uint8Array | null;
};

export function decodeOutboundQueuedTransfer(
  value: unknown
): OutboundQueuedTransfer {
  const s = struct(value, "OutboundQueuedTransfer");
  return {
    sender: field(s, "sender", isString),
    amount: decodeTrimmedAmount(s["amount"]),
    recipientChain: field(s, "recipient_chain", isNumber),
    recipientNttManager: field(s, "recipient_ntt_manager", isBytes),
    recipient: field(s, "recipient", isBytes),
    sourceToken: field(s, "source_token", isBytes),
    releaseTimestamp: field(s, "release_timestamp", isBigint),
    additionalPayload: optionalField(s, "additional_payload", isBytes),
  };
}

export type TransceiverInfo = {
  address: string;
  enabled: boolean;
  /** Permanent bitmap index (0-63); never reused, even after removal. */
  index: number;
};

export function decodeTransceiverInfo(value: unknown): TransceiverInfo {
  const s = struct(value, "TransceiverInfo");
  return {
    address: field(s, "address", isString),
    enabled: field(s, "enabled", isBoolean),
    index: field(s, "index", isNumber),
  };
}

export type AttestationInfo = {
  executed: boolean;
  /** Bitmap of the transceiver indices that have attested. */
  attestedTransceivers: bigint;
};

export function decodeAttestationInfo(value: unknown): AttestationInfo {
  const s = struct(value, "AttestationInfo");
  return {
    executed: field(s, "executed", isBoolean),
    attestedTransceivers: field(s, "attested_transceivers", isBigint),
  };
}

export type TransceiverFee = {
  transceiver: string;
  /** `null` when that transceiver could not produce a quote. */
  fee: bigint | null;
};

export function decodeTransceiverFee(value: unknown): TransceiverFee {
  const s = struct(value, "TransceiverFee");
  return {
    transceiver: field(s, "transceiver", isString),
    fee: optionalField(s, "fee", isBigint),
  };
}
