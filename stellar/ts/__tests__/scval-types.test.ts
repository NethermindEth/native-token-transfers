import {
  Address,
  nativeToScVal,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import {
  decodeAttestationInfo,
  decodeInboundQueuedTransfer,
  decodeMode,
  decodeNttManagerPeer,
  decodeOutboundQueuedTransfer,
  decodePeerInfo,
  decodeTransceiverFee,
  decodeTransceiverInfo,
  decodeTransferResult,
  structArg,
} from "../src/scval-types.js";

// A `#[contracttype]` struct is an ScMap keyed by symbols, so build the
// fixtures that way and let the SDK produce the decoder's input: this pins the
// JS types `scValToNative` yields (u32 -> number, u64/i128 -> bigint,
// BytesN -> Buffer, Address -> StrKey string, Option::None -> null), not just
// the field names.
const struct = (fields: Record<string, xdr.ScVal>): unknown =>
  scValToNative(
    xdr.ScVal.scvMap(
      Object.entries(fields).map(
        ([name, val]) =>
          new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(name), val })
      )
    )
  );

const u32 = (v: number) => nativeToScVal(v, { type: "u32" });
const u64 = (v: bigint) => nativeToScVal(v, { type: "u64" });
const i128 = (v: bigint) => nativeToScVal(v, { type: "i128" });
const bytes = (fill: number) =>
  nativeToScVal(Buffer.alloc(32, fill), { type: "bytes" });

const ACCOUNT = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";
const CONTRACT = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";

describe("ScVal decoders", () => {
  it("decodes Mode from its u32 discriminant", () => {
    expect(decodeMode(scValToNative(u32(0)))).toEqual("locking");
    expect(decodeMode(scValToNative(u32(1)))).toEqual("burning");
    expect(() => decodeMode(scValToNative(u32(2)))).toThrow();
  });

  it("decodes NttManagerPeer and its nested rate limiter", () => {
    const peer = struct({
      address: bytes(0xab),
      token_decimals: u32(6),
      inbound_rate_limit: xdr.ScVal.scvMap([
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol("limit"),
          val: u64(1000n),
        }),
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol("current_capacity"),
          val: u64(900n),
        }),
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol("last_tx_timestamp"),
          val: u64(1700000000n),
        }),
      ]),
    });

    expect(decodeNttManagerPeer(peer)).toEqual({
      address: Buffer.alloc(32, 0xab),
      tokenDecimals: 6,
      inboundRateLimit: {
        limit: 1000n,
        currentCapacity: 900n,
        lastTxTimestamp: 1700000000n,
      },
    });
  });

  it("decodes TransferResult", () => {
    const result = struct({
      sequence: u64(7n),
      queued: xdr.ScVal.scvBool(true),
      digest: bytes(0x01),
    });

    expect(decodeTransferResult(result)).toEqual({
      sequence: 7n,
      queued: true,
      digest: Buffer.alloc(32, 0x01),
    });
  });

  it("decodes both queued-transfer shapes", () => {
    const inbound = struct({
      recipient: new Address(ACCOUNT).toScVal(),
      amount: i128(500n),
      trimmed_amount: u64(5n),
      release_timestamp: u64(1700000042n),
    });

    expect(decodeInboundQueuedTransfer(inbound)).toEqual({
      recipient: ACCOUNT,
      amount: 500n,
      trimmedAmount: 5n,
      releaseTimestamp: 1700000042n,
    });

    const outbound = struct({
      sender: new Address(ACCOUNT).toScVal(),
      amount: xdr.ScVal.scvMap([
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol("amount"),
          val: u64(5n),
        }),
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol("decimals"),
          val: u32(7),
        }),
      ]),
      recipient_chain: u32(2),
      recipient_ntt_manager: bytes(0x02),
      recipient: bytes(0x03),
      source_token: bytes(0x04),
      release_timestamp: u64(1700000042n),
      additional_payload: xdr.ScVal.scvVoid(),
    });

    expect(decodeOutboundQueuedTransfer(outbound)).toEqual({
      sender: ACCOUNT,
      amount: { amount: 5n, decimals: 7 },
      recipientChain: 2,
      recipientNttManager: Buffer.alloc(32, 0x02),
      recipient: Buffer.alloc(32, 0x03),
      sourceToken: Buffer.alloc(32, 0x04),
      releaseTimestamp: 1700000042n,
      additionalPayload: null,
    });
  });

  it("decodes TransceiverInfo and AttestationInfo", () => {
    const info = struct({
      address: new Address(CONTRACT).toScVal(),
      enabled: xdr.ScVal.scvBool(true),
      index: u32(0),
    });
    expect(decodeTransceiverInfo(info)).toEqual({
      address: CONTRACT,
      enabled: true,
      index: 0,
    });

    const attestation = struct({
      executed: xdr.ScVal.scvBool(false),
      attested_transceivers: u64(0b101n),
    });
    expect(decodeAttestationInfo(attestation)).toEqual({
      executed: false,
      attestedTransceivers: 0b101n,
    });
  });

  it("distinguishes a quoted fee from an unavailable one", () => {
    const quoted = struct({
      transceiver: new Address(CONTRACT).toScVal(),
      fee: i128(100n),
    });
    expect(decodeTransceiverFee(quoted)).toEqual({
      transceiver: CONTRACT,
      fee: 100n,
    });

    const unavailable = struct({
      transceiver: new Address(CONTRACT).toScVal(),
      fee: xdr.ScVal.scvVoid(),
    });
    expect(decodeTransceiverFee(unavailable)).toEqual({
      transceiver: CONTRACT,
      fee: null,
    });
  });

  it("decodes a transceiver PeerInfo", () => {
    const peer = struct({
      emitter: bytes(0xcd),
      enabled: xdr.ScVal.scvBool(false),
    });
    expect(decodePeerInfo(peer)).toEqual({
      emitter: Buffer.alloc(32, 0xcd),
      // A disabled peer keeps its emitter, so both fields have to survive.
      enabled: false,
    });
  });

  it("rejects a value that is not the expected struct", () => {
    expect(() => decodeTransferResult(null)).toThrow(/TransferResult/);
    expect(() => decodeTransferResult(struct({ sequence: u64(7n) }))).toThrow(
      /queued/
    );
    expect(() =>
      decodeTransceiverFee(
        struct({ transceiver: new Address(CONTRACT).toScVal() })
      )
    ).toThrow(/fee/);
  });
});

describe("ScVal encoders", () => {
  it("keys a struct argument by symbol, in sorted order", () => {
    // `scValToNative` reads a string key and a symbol key back the same way, so
    // the round-trip cannot catch this: the host is what rejects an scvString.
    // `ExecutorArgs`, declared in the order the Rust struct declares it. The
    // host wants its map keys in byte order and `nativeToScVal` sorts them with
    // `localeCompare`, so the five real names are what has to agree.
    const entries = structArg({
      payee: new Address(ACCOUNT).toScVal(),
      amount: i128(1n),
      refund: new Address(ACCOUNT).toScVal(),
      signed_quote: bytes(0x01),
      relay_instructions: bytes(0x02),
    }).map()!;

    expect(entries.map((e) => e.key().switch().name)).toEqual(
      new Array(5).fill("scvSymbol")
    );
    expect(entries.map((e) => e.key().sym().toString())).toEqual([
      "amount",
      "payee",
      "refund",
      "relay_instructions",
      "signed_quote",
    ]);
    expect(scValToNative(structArg({ dbps: u32(500) }))).toEqual({ dbps: 500 });
  });
});
