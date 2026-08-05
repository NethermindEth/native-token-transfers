// ESM Jest does not inject the `jest` global; import it.
import { jest } from "@jest/globals";
import type { rpc as SorobanRpc, xdr } from "@stellar/stellar-sdk";
import {
  UniversalAddress,
  createVAA,
} from "@wormhole-foundation/sdk-definitions";
import { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import {
  StellarAddress,
  StellarPlatform,
} from "@wormhole-foundation/sdk-stellar";
import { StellarNtt } from "../src/ntt.js";

const MANAGER = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
const TOKEN = "CA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";
const TRANSCEIVER = "CBQCJMQTPGGDHNCLJKKJ7XLNKQAGXPKNXHLJTQPKQOB5QUAQTQZ7B6JG";
const CORE = "CDCPB4NPHRIF3W7TPQAXCFCTZL2NEDGJJ3RIFH3JBM7MKMAAADYSJRQK";
const OWNER = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";

const contracts = {
  coreBridge: CORE,
  ntt: {
    manager: MANAGER,
    token: TOKEN,
    transceiver: { wormhole: TRANSCEIVER },
  },
};

// The manager's read surface, keyed by contract method. `simulateRead` has
// already run `scValToNative`, so these are the shapes it hands back.
const managerState: Record<string, unknown> = {
  get_mode: 0,
  paused: false,
  get_owner: OWNER,
  get_pauser: null,
  get_threshold: 1,
  token_decimals: 7,
  get_chain_id: 61,
  get_token: TOKEN,
  get_transceiver_info: { address: TRANSCEIVER, enabled: true, index: 0 },
  get_peer: {
    address: Buffer.alloc(32, 0xab),
    token_decimals: 18,
    inbound_rate_limit: {
      limit: 1000n,
      current_capacity: 900n,
      last_tx_timestamp: 1700000000n,
    },
  },
  get_outbound_capacity: 900n,
  get_outbound_limit_params: {
    limit: 1000n,
    current_capacity: 900n,
    last_tx_timestamp: 1700000000n,
  },
  get_inbound_capacity: 500n,
  get_inbound_limit_params: {
    limit: 800n,
    current_capacity: 500n,
    last_tx_timestamp: 1700000000n,
  },
  get_rate_limit_duration: 86400n,
  get_inbound_queue_item: {
    recipient: OWNER,
    amount: 500n,
    trimmed_amount: 5n,
    release_timestamp: 1700000042n,
  },
  get_outbound_queue_item: null,
};

const message: Ntt.Message = {
  id: new Uint8Array(32),
  sender: new UniversalAddress(new Uint8Array(32)),
  payload: {
    trimmedAmount: { amount: 1n, decimals: 7 },
    sourceToken: new UniversalAddress(new Uint8Array(32)),
    recipientAddress: new UniversalAddress(new Uint8Array(32)),
    recipientChain: "Stellar",
    additionalPayload: new Uint8Array(),
  },
};

const attestation = createVAA("Ntt:WormholeTransfer", {
  emitterChain: "Ethereum",
  emitterAddress: new UniversalAddress(new Uint8Array(32)),
  sequence: 1n,
  guardianSet: 0,
  timestamp: 0,
  consistencyLevel: 1,
  nonce: 0,
  signatures: [],
  payload: {
    sourceNttManager: new UniversalAddress(new Uint8Array(32)),
    recipientNttManager: new UniversalAddress(new Uint8Array(32)),
    nttManagerPayload: message,
  },
});

let overrides: Record<string, unknown>;
let calls: { method: string; args: xdr.ScVal[] }[];

const ntt = () =>
  new StellarNtt("Testnet", "Stellar", {} as SorobanRpc.Server, contracts);

beforeEach(() => {
  overrides = {};
  calls = [];
  jest
    .spyOn(StellarPlatform, "simulateRead")
    .mockImplementation(async (_rpc, _network, contractId, method, ...args) => {
      expect(contractId).toEqual(MANAGER);
      calls.push({ method, args });
      const state = method in overrides ? overrides : managerState;
      if (!(method in state)) throw new Error(`unstubbed read: ${method}`);
      return state[method];
    });
});

afterEach(() => jest.restoreAllMocks());

describe("StellarNtt construction", () => {
  it("rejects a config missing the pieces every call needs", () => {
    expect(
      () =>
        new StellarNtt("Testnet", "Stellar", {} as SorobanRpc.Server, {
          coreBridge: CORE,
        })
    ).toThrow(/NTT contracts/);
    expect(
      () =>
        new StellarNtt("Testnet", "Stellar", {} as SorobanRpc.Server, {
          ntt: contracts.ntt,
        })
    ).toThrow(/CoreBridge/);
  });

  it("accepts a manager whose transceiver is not registered yet", () => {
    // verifyAddresses is what reports that, so it has to be constructible.
    expect(
      new StellarNtt("Testnet", "Stellar", {} as SorobanRpc.Server, {
        coreBridge: CORE,
        ntt: { ...contracts.ntt, transceiver: {} },
      }).transceiverAddress
    ).toBeUndefined();
  });
});

describe("StellarNtt config getters", () => {
  it("reads the manager's configuration", async () => {
    const n = ntt();
    await expect(n.getMode()).resolves.toEqual("locking");
    await expect(n.isPaused()).resolves.toEqual(false);
    await expect(n.getThreshold()).resolves.toEqual(1);
    await expect(n.getTokenDecimals()).resolves.toEqual(7);
    await expect(n.getChainId()).resolves.toEqual(61);
    await expect(n.getCustodyAddress()).resolves.toEqual(MANAGER);
    expect((await n.getOwner()).toString()).toEqual(OWNER);
    await expect(n.getPauser()).resolves.toBeNull();
  });

  it("surfaces a renounced owner instead of returning null", async () => {
    overrides = { get_owner: null };
    await expect(ntt().getOwner()).rejects.toThrow(/renounced/);
  });

  it("rejects a return value of the wrong type", async () => {
    overrides = { get_threshold: 1n };
    await expect(ntt().getThreshold()).rejects.toThrow(/u32/);
  });

  it("returns a peer's wire address as universal", async () => {
    // The peer's 32 bytes are opaque here — on a Stellar peer they are a
    // one-way hash_address, so they must not be read back as a native address.
    await expect(ntt().getPeer("Ethereum")).resolves.toEqual({
      address: {
        chain: "Ethereum",
        address: new UniversalAddress(new Uint8Array(Buffer.alloc(32, 0xab))),
      },
      tokenDecimals: 18,
      inboundLimit: 1000n,
    });
  });

  it("returns null for an unregistered peer", async () => {
    overrides = { get_peer: null };
    await expect(ntt().getPeer("Ethereum")).resolves.toBeNull();
  });
});

describe("StellarNtt rate limits", () => {
  it("reads outbound capacity, limits and the refill window", async () => {
    const n = ntt();
    // token_decimals is 7, so the trimmed domain is already the token's.
    await expect(n.getCurrentOutboundCapacity()).resolves.toEqual(900n);
    await expect(n.getOutboundLimit()).resolves.toEqual(1000n);
    // Seconds, not milliseconds — Soroban ledger time.
    await expect(n.getRateLimitDuration()).resolves.toEqual(86400n);
  });

  it("reads per-peer inbound capacity and limits", async () => {
    const n = ntt();
    await expect(n.getCurrentInboundCapacity("Ethereum")).resolves.toEqual(
      500n
    );
    await expect(n.getInboundLimit("Ethereum")).resolves.toEqual(800n);
    expect(
      calls
        .filter((c) => c.method.startsWith("get_inbound"))
        .map((c) => c.args[0]!.u32())
    ).toEqual([2, 2]);
  });

  it("rescales limits out of the trimmed domain into token decimals", async () => {
    // The manager consumes rate limits in trimmed units (min(8, decimals)) and
    // stores a bare u64, so an 18-decimal token needs 10 decimals put back.
    overrides = { token_decimals: 18 };
    const n = ntt();
    await expect(n.getCurrentOutboundCapacity()).resolves.toEqual(
      900n * 10n ** 10n
    );
    await expect(n.getOutboundLimit()).resolves.toEqual(1000n * 10n ** 10n);
    await expect(n.getCurrentInboundCapacity("Ethereum")).resolves.toEqual(
      500n * 10n ** 10n
    );
    await expect(n.getInboundLimit("Ethereum")).resolves.toEqual(
      800n * 10n ** 10n
    );
    expect((await n.getPeer("Ethereum"))!.inboundLimit).toEqual(
      1000n * 10n ** 10n
    );
  });

  it("reports no capacity for a chain that is not a peer", async () => {
    overrides = { get_inbound_capacity: null, get_inbound_limit_params: null };
    const n = ntt();
    await expect(n.getCurrentInboundCapacity("Ethereum")).resolves.toEqual(0n);
    await expect(n.getInboundLimit("Ethereum")).resolves.toEqual(0n);
  });

  it("keys the inbound queue by the locally computed digest", async () => {
    await expect(
      ntt().getInboundQueuedTransfer("Ethereum", message)
    ).resolves.toEqual({
      recipient: new StellarAddress(OWNER),
      amount: 500n,
      rateLimitExpiryTimestamp: 1700000042,
    });
    expect(new Uint8Array(calls[0]!.args[0]!.bytes())).toEqual(
      Ntt.messageDigest("Ethereum", message)
    );
  });

  it("returns null for a transfer that was never queued", async () => {
    overrides = { get_inbound_queue_item: null };
    await expect(
      ntt().getInboundQueuedTransfer("Ethereum", message)
    ).resolves.toBeNull();
    await expect(ntt().getOutboundQueuedTransfer(1n)).resolves.toBeNull();
  });
});

describe("StellarNtt attestation status", () => {
  it("keys the approval read by the digest of the manager message", async () => {
    overrides = { is_message_approved: true };
    await expect(ntt().getIsApproved(attestation)).resolves.toEqual(true);
    expect(new Uint8Array(calls[0]!.args[0]!.bytes())).toEqual(
      Ntt.messageDigest("Ethereum", message)
    );
  });

  it("does not report a queued transfer as executed", async () => {
    // The manager marks the attestation executed when it queues the transfer,
    // so the queue has to be checked too before calling the transfer complete.
    overrides = { is_message_executed: true };
    await expect(ntt().getIsExecuted(attestation)).resolves.toEqual(false);
    await expect(
      ntt().getIsTransferInboundQueued(attestation)
    ).resolves.toEqual(true);

    overrides = { is_message_executed: true, get_inbound_queue_item: null };
    await expect(ntt().getIsExecuted(attestation)).resolves.toEqual(true);
  });

  it("skips the queue lookup when the message was never executed", async () => {
    overrides = { is_message_executed: false };
    await expect(ntt().getIsExecuted(attestation)).resolves.toEqual(false);
    expect(calls.map((c) => c.method)).toEqual(["is_message_executed"]);
  });
});

describe("StellarNtt delivery quotes", () => {
  const options = { queue: false };

  it("sums the quotes of every enabled transceiver", async () => {
    overrides = {
      quote_delivery_price: [
        { transceiver: TRANSCEIVER, fee: 100n },
        { transceiver: MANAGER, fee: 250n },
      ],
    };
    await expect(
      ntt().quoteDeliveryPrice("Ethereum", options)
    ).resolves.toEqual(350n);
    expect(calls[0]!.args[0]!.u32()).toEqual(2);
  });

  it("skips a transceiver that could not quote", async () => {
    overrides = {
      quote_delivery_price: [
        { transceiver: TRANSCEIVER, fee: null },
        { transceiver: MANAGER, fee: 250n },
      ],
    };
    await expect(
      ntt().quoteDeliveryPrice("Ethereum", options)
    ).resolves.toEqual(250n);
  });

  it("never advertises relaying: Stellar has no quoter", async () => {
    await expect(ntt().isRelayingAvailable("Ethereum")).resolves.toEqual(false);
    // ...so asking for an automatic quote is a mistake, not a manual price.
    await expect(
      ntt().quoteDeliveryPrice("Ethereum", { queue: false, automatic: true })
    ).rejects.toThrow(/not available/);
  });
});

describe("StellarNtt.verifyAddresses", () => {
  it("returns null when the on-chain addresses match the config", async () => {
    await expect(ntt().verifyAddresses()).resolves.toBeNull();
  });

  it("reports only the addresses that differ", async () => {
    overrides = { get_token: OWNER };
    await expect(ntt().verifyAddresses()).resolves.toEqual({ token: OWNER });
  });

  it("reports an unregistered transceiver as a mismatch", async () => {
    overrides = { get_transceiver_info: null };
    await expect(ntt().verifyAddresses()).resolves.toEqual({ transceiver: {} });
  });
});
