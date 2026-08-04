// ESM Jest does not inject the `jest` global; import it.
import { jest } from "@jest/globals";
import type { rpc as SorobanRpc } from "@stellar/stellar-sdk";
import { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import { StellarPlatform } from "@wormhole-foundation/sdk-stellar";
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
};

let overrides: Record<string, unknown>;

const ntt = () =>
  new StellarNtt("Testnet", "Stellar", {} as SorobanRpc.Server, contracts);

beforeEach(() => {
  overrides = {};
  jest
    .spyOn(StellarPlatform, "simulateRead")
    .mockImplementation(async (_rpc, _network, contractId, method) => {
      expect(contractId).toEqual(MANAGER);
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
