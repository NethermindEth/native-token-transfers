// ESM Jest does not inject the `jest` global; import it.
import { jest } from "@jest/globals";
import {
  Account,
  Address,
  Operation,
  SorobanDataBuilder,
  nativeToScVal,
  scValToNative,
  type Transaction,
  rpc as SorobanRpc,
  xdr,
} from "@stellar/stellar-sdk";
import { encoding } from "@wormhole-foundation/sdk-base";
import type { ChainAddress } from "@wormhole-foundation/sdk-definitions";
import { serialize } from "@wormhole-foundation/sdk-definitions";
import {
  UniversalAddress,
  createVAA,
} from "@wormhole-foundation/sdk-definitions";
import { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import {
  StellarAddress,
  StellarPlatform,
  nativeSacId,
  type StellarUnsignedTransaction,
} from "@wormhole-foundation/sdk-stellar";
import { StellarNtt } from "../src/ntt.js";
import { StellarNttWormholeTransceiver } from "../src/transceiver.js";

const MANAGER = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
const TOKEN = "CBD3J5AK3ZNX3CJVELGW2N32P3CI2TLMRIQEIL5OXKWA2HH2LN2DA3AO";
const TRANSCEIVER = "CBOBIVCYTPASAMHAKQVIXN73PLY6MFJQSWJCPLCFGDXW2ZBUC3RCZN5S";
const CORE = "CDKTO4XRUS3SHEFSYEAAGVR5MCWIYDR5EFUVH5AUZXX5MAEM4KB5BSED";
const OWNER = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";
const PAUSER = "GC4E6RZNOREPKUIGQR3BLPNGPUT3MAPLZV4LGF7UTLR2G5JKHKVYQVHZ";

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
  get_enabled_bitmap: 1n,
};

/** The Wormhole core's read surface. */
const coreState: Record<string, unknown> = {
  get_message_fee: 100n,
};

/** The transceiver's own read surface, keyed the same way. */
const transceiverState: Record<string, unknown> = {
  get_transceiver_type: Buffer.from("wormhole"),
  get_peer: Buffer.alloc(32, 0xcd),
  get_peer_info: { emitter: Buffer.alloc(32, 0xcd), enabled: true },
  is_peer_enabled: true,
  is_vaa_consumed: false,
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
/**
 * The auth entries the simulation records, given the call a write makes; none
 * by default, as for a write that authorizes nothing.
 */
let recordAuth: (
  call: xdr.InvokeContractArgs
) => xdr.SorobanAuthorizationEntry[];
/** The resource fee the simulation charges, in stroops. */
let resourceFee: number;

/** The RPC's simulation of `tx`, recording `recordAuth`'s entries. */
const simulation = (tx: Transaction) => {
  const [operation] = tx.operations as [Operation.InvokeHostFunction];
  const auth = recordAuth(operation.func.invokeContract());
  return {
    id: "1",
    latestLedger: 1000,
    transactionData: new SorobanDataBuilder()
      .setResourceFee(resourceFee)
      .build()
      .toXDR("base64"),
    minResourceFee: String(resourceFee),
    results: [
      {
        auth: auth.map((entry) => entry.toXDR("base64")),
        xdr: xdr.ScVal.scvVoid().toXDR("base64"),
      },
    ],
  };
};

// Writes only need the rpc calls `prepare` makes. `prepareTransaction`
// assembles the built transaction with the simulation, the way the real one
// does, so the tests can inspect it.
const provider = {
  getAccount: async (address: string) => new Account(address, "17"),
  prepareTransaction: async (tx: Transaction) =>
    SorobanRpc.assembleTransaction(tx, simulation(tx)).build(),
  getLatestLedger: async () => ({ sequence: 1000 }),
} as unknown as SorobanRpc.Server;

/** The arguments of a call to `method` on `contract`. */
const contractCall = (contract: string, method: string, ...args: xdr.ScVal[]) =>
  new xdr.InvokeContractArgs({
    contractAddress: new Address(contract).toScAddress(),
    functionName: method,
    args,
  });

/** An auth tree rooted at `call`, the way a simulation records it. */
const authorized = (
  call: xdr.InvokeContractArgs,
  ...children: xdr.SorobanAuthorizedInvocation[]
): xdr.SorobanAuthorizedInvocation =>
  new xdr.SorobanAuthorizedInvocation({
    function:
      xdr.SorobanAuthorizedFunction.sorobanAuthorizedFunctionTypeContractFn(
        call
      ),
    subInvocations: children,
  });

/**
 * Records one source-account entry rooted at the write's own call, with
 * `children` below it: what a simulation returns when the source account is
 * the address the call authorizes.
 */
const sourceAuth =
  (...children: xdr.SorobanAuthorizedInvocation[]) =>
  (call: xdr.InvokeContractArgs) => [
    new xdr.SorobanAuthorizationEntry({
      credentials: xdr.SorobanCredentials.sorobanCredentialsSourceAccount(),
      rootInvocation: authorized(call, ...children),
    }),
  ];

const ntt = () => new StellarNtt("Testnet", "Stellar", provider, contracts);

/** The contract, method, and decoded arguments a yielded transaction invokes. */
const invocation = (tx: StellarUnsignedTransaction<"Testnet", "Stellar">) => {
  const [operation] = tx.transaction.operations;
  const invoke = (
    operation as Operation.InvokeHostFunction
  ).func.invokeContract();
  return {
    contract: Address.fromScAddress(invoke.contractAddress()).toString(),
    method: invoke.functionName().toString(),
    args: invoke.args().map((arg) => scValToNative(arg)),
  };
};

const collect = async (
  txs: AsyncGenerator<StellarUnsignedTransaction<"Testnet", "Stellar">>
) => {
  const collected = [];
  for await (const tx of txs) collected.push(tx);
  return collected;
};

/** The single invocation a one-transaction write yields. */
const one = async (
  txs: AsyncGenerator<StellarUnsignedTransaction<"Testnet", "Stellar">>
) => invocation((await collect(txs))[0]!);

beforeEach(() => {
  overrides = {};
  calls = [];
  recordAuth = () => [];
  resourceFee = 0;
  jest
    .spyOn(StellarPlatform, "simulateRead")
    .mockImplementation(async (_rpc, _network, contractId, method, ...args) => {
      calls.push({ method, args });
      const contract =
        contractId === MANAGER
          ? managerState
          : contractId === CORE
            ? coreState
            : transceiverState;
      const state = method in overrides ? overrides : contract;
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
    // `verifyAddresses` is what reports that, so it has to be constructible.
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

  it("reads the token decimals on every call", async () => {
    const n = ntt();
    await n.getTokenDecimals();
    overrides = { token_decimals: 9 };
    await expect(n.getTokenDecimals()).resolves.toEqual(9);
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
    // one-way `hash_address`, so they must not be read back as a native
    // address.
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
    // `token_decimals` is 7, so the trimmed domain is already the token's.
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

  it("fails when a transceiver could not quote", async () => {
    overrides = {
      quote_delivery_price: [
        { transceiver: TRANSCEIVER, fee: null },
        { transceiver: MANAGER, fee: 250n },
      ],
    };
    await expect(ntt().quoteDeliveryPrice("Ethereum", options)).rejects.toThrow(
      `Transceiver ${TRANSCEIVER} could not quote a fee`
    );
  });

  it("never advertises relaying: Stellar has no quoter", async () => {
    await expect(ntt().isRelayingAvailable("Ethereum")).resolves.toEqual(false);
    // ...so asking for an automatic quote is a mistake, not a manual price.
    await expect(
      ntt().quoteDeliveryPrice("Ethereum", { queue: false, automatic: true })
    ).rejects.toThrow(/not available/);
  });
});

describe("StellarNtt.transfer", () => {
  const recipient = new UniversalAddress(new Uint8Array(32).fill(0x11));
  const destination = {
    chain: "Ethereum",
    address: recipient,
  } as ChainAddress;

  it("sends the recipient's universal address to the manager", async () => {
    const [tx, ...rest] = await collect(
      ntt().transfer(new StellarAddress(OWNER), 42n, destination, {
        queue: false,
      })
    );
    expect(rest).toEqual([]);
    expect(invocation(tx!)).toEqual({
      contract: MANAGER,
      method: "transfer",
      // `sender`, `amount`, `recipient_chain`, `recipient`, `should_queue`.
      // The manager hashes the sender itself, so it goes over the ABI as an
      // `Address`.
      args: [OWNER, 42n, 2, Buffer.from(recipient.toUint8Array()), false],
    });
    // Sequential, so a queued transfer's completion lands after the transfer.
    expect(tx!.parallelizable).toEqual(false);
    expect(tx!.description).toEqual("Ntt.transfer");
  });

  it("routes an additional payload through transfer_with_payload", async () => {
    const payload = new Uint8Array([1, 2, 3]);
    const [tx] = await collect(
      ntt().transfer(
        new StellarAddress(OWNER),
        42n,
        destination,
        { queue: true },
        payload
      )
    );
    expect(invocation(tx!).method).toEqual("transfer_with_payload");
    expect(invocation(tx!).args).toEqual([
      OWNER,
      42n,
      2,
      Buffer.from(recipient.toUint8Array()),
      true,
      Buffer.from(payload),
    ]);
  });

  it("refuses an automatic transfer and a sender it cannot sign for", async () => {
    await expect(
      collect(
        ntt().transfer(new StellarAddress(OWNER), 42n, destination, {
          queue: false,
          automatic: true,
        })
      )
    ).rejects.toThrow(/not available/);
    // A `UniversalAddress` is a one-way `hash_address`; it cannot source a tx.
    await expect(
      collect(ntt().transfer(recipient, 42n, destination, { queue: false }))
    ).rejects.toThrow();
  });
});

describe("StellarNtt admin setters", () => {
  const owner = new StellarAddress(OWNER);
  const peer = {
    chain: "Ethereum",
    address: new UniversalAddress(new Uint8Array(32).fill(0x11)),
  } as ChainAddress;
  const peerBytes = Buffer.from(
    peer.address.toUniversalAddress().toUint8Array()
  );

  it("registers a peer and its transceiver counterpart", async () => {
    expect(await one(ntt().setPeer(peer, 18, 1000n, owner))).toEqual({
      contract: MANAGER,
      method: "set_peer",
      args: [2, peerBytes, 18, 1000n],
    });
    // The transceiver's peer is the emitter its VAAs carry, and is one-shot.
    expect(await one(ntt().setTransceiverPeer(0, peer, owner))).toEqual({
      contract: TRANSCEIVER,
      method: "set_peer",
      args: [2, peerBytes],
    });
  });

  it("manages the threshold and the transceiver registry", async () => {
    expect(await one(ntt().setThreshold(2, owner))).toEqual({
      contract: MANAGER,
      method: "set_threshold",
      args: [2],
    });
    expect(await one(ntt().setTransceiver(TRANSCEIVER, owner))).toEqual({
      contract: MANAGER,
      method: "set_transceiver",
      args: [TRANSCEIVER],
    });
    expect(await one(ntt().removeTransceiver(TRANSCEIVER, owner))).toEqual({
      contract: MANAGER,
      method: "remove_transceiver",
      args: [TRANSCEIVER],
    });
  });

  it("hands over ownership in two steps and pauses in one", async () => {
    const proposed = new StellarAddress(PAUSER);
    expect(await one(ntt().setOwner(proposed, owner))).toEqual({
      contract: MANAGER,
      method: "transfer_ownership",
      // Defaulted from the latest ledger, so a stale offer expires by itself.
      args: [PAUSER, 1000 + 17280],
    });
    expect(await one(ntt().setOwner(proposed, owner, 0))).toEqual({
      contract: MANAGER,
      method: "transfer_ownership",
      args: [PAUSER, 0],
    });
    expect(await one(ntt().acceptOwnership(proposed))).toEqual({
      contract: MANAGER,
      method: "accept_ownership",
      args: [],
    });

    expect(await one(ntt().setPauser(proposed, owner))).toEqual({
      contract: MANAGER,
      method: "transfer_pauser",
      args: [OWNER, PAUSER],
    });
    // `None` gives up the role, leaving pause owner-only.
    expect((await one(ntt().setPauser(null, owner))).args).toEqual([
      OWNER,
      null,
    ]);
    expect(await one(ntt().pause(owner))).toEqual({
      contract: MANAGER,
      method: "pause",
      args: [OWNER],
    });
    expect((await one(ntt().unpause(owner))).method).toEqual("unpause");
  });

  it("trims rate limits into the domain the manager stores", async () => {
    // The manager stores a bare u64 in the trimmed domain, so an 18-decimal
    // token's limit has to lose 10 decimals on the way in — the exact inverse
    // of what the reads put back, or every limit lands 10^10 too high.
    overrides = { token_decimals: 18 };
    const limit = 1000n * 10n ** 10n;
    expect((await one(ntt().setOutboundLimit(limit, owner))).args).toEqual([
      1000n,
    ]);
    expect(
      (await one(ntt().setInboundLimit("Ethereum", limit, owner))).args
    ).toEqual([2, 1000n]);
    expect((await one(ntt().setPeer(peer, 18, limit, owner))).args[3]).toEqual(
      1000n
    );
  });

  it("round-trips a limit through set and read", async () => {
    overrides = { token_decimals: 18 };
    const n = ntt();
    const [stored] = (await one(n.setOutboundLimit(4_200n * 10n ** 10n, owner)))
      .args as [bigint];
    overrides = {
      token_decimals: 18,
      get_outbound_limit_params: {
        limit: stored,
        current_capacity: stored,
        last_tx_timestamp: 0n,
      },
    };
    await expect(n.getOutboundLimit()).resolves.toEqual(4_200n * 10n ** 10n);
  });

  it("refuses a limit the manager cannot represent", async () => {
    // 7 decimals, so nothing is trimmed away and the u64 is the real ceiling.
    await expect(
      collect(ntt().setOutboundLimit(2n ** 64n, owner))
    ).rejects.toThrow(/overflows/);

    // The other end: a limit under one trimmed unit floors to zero, which the
    // rate limiter reads as "allow nothing" rather than "allow a little".
    overrides = { token_decimals: 18 };
    await expect(collect(ntt().setOutboundLimit(1n, owner))).rejects.toThrow(
      /below one trimmed unit/
    );
  });

  it("refuses an admin call nobody can source", async () => {
    await expect(collect(ntt().setThreshold(1))).rejects.toThrow(
      /no implicit signer/
    );
    // Only a G… account can source a transaction; a contract owner has to
    // administer the manager by invoking itself.
    await expect(
      collect(ntt().setThreshold(1, new StellarAddress(TOKEN)))
    ).rejects.toThrow(/must be a G… account/);
  });
});

describe("StellarNtt.redeem", () => {
  it("hands each VAA to the transceiver in its own transaction", async () => {
    const txs = await collect(
      ntt().redeem([attestation, attestation], new StellarAddress(OWNER))
    );
    expect(txs.map((tx) => invocation(tx))).toEqual(
      Array(2).fill({
        contract: TRANSCEIVER,
        method: "receive_message",
        args: [Buffer.from(serialize(attestation))],
      })
    );
  });

  it("points a rejected message at the address registry", async () => {
    // `flatten_call` masks the manager's own error as `ManagerRejectedMessage`,
    // so the inner code is the only evidence of what actually went wrong.
    const failing = new StellarNtt(
      "Testnet",
      "Stellar",
      {
        ...provider,
        prepareTransaction: async () => {
          throw new Error(
            "HostError: Error(Contract, #36)\nError(Contract, #66)"
          );
        },
      } as unknown as SorobanRpc.Server,
      contracts
    );

    const rejected = await collect(
      failing.redeem([attestation], new StellarAddress(OWNER))
    ).catch((error: Error) => error);

    expect((rejected as Error).message).toMatch(
      /RecipientNotRegistered.*recordAddress/s
    );
    // Named once, by `prepare`, not again by the guidance layered on top.
    expect(
      (rejected as Error).message.match(/ManagerRejectedMessage/g)
    ).toHaveLength(1);
  });

  it("refuses to redeem without a transceiver to redeem through", async () => {
    const unwired = new StellarNtt("Testnet", "Stellar", provider, {
      coreBridge: CORE,
      ntt: { ...contracts.ntt, transceiver: {} },
    });
    await expect(
      collect(unwired.redeem([attestation], new StellarAddress(OWNER)))
    ).rejects.toThrow(/No transceiver at index 0/);
  });
});

describe("StellarNtt.recordAddress", () => {
  it("registers an address on the core so its hash can be resolved", async () => {
    const [tx] = await collect(
      ntt().recordAddress(new StellarAddress(OWNER), TOKEN)
    );
    expect(invocation(tx!)).toEqual({
      contract: CORE,
      method: "record_address",
      args: [TOKEN],
    });
  });
});

describe("StellarNtt queue completion", () => {
  it("keys the inbound completion by the message digest", async () => {
    const [tx] = await collect(
      ntt().completeInboundQueuedTransfer(
        "Ethereum",
        message,
        new StellarAddress(OWNER)
      )
    );
    expect(invocation(tx!)).toEqual({
      contract: MANAGER,
      method: "complete_inbound_transfer",
      args: [Buffer.from(Ntt.messageDigest("Ethereum", message))],
    });
  });

  it("keys the outbound queue by sequence, not digest", async () => {
    const payer = new StellarAddress(OWNER);
    const [completed] = await collect(
      ntt().completeOutboundQueuedTransfer(7n, payer)
    );
    expect(invocation(completed!)).toEqual({
      contract: MANAGER,
      method: "complete_queued_transfer",
      args: [OWNER, 7n],
    });

    // Canceling is sender-gated, so the sender goes over the ABI as well.
    const [cancelled] = await collect(
      ntt().cancelOutboundQueuedTransfer(7n, payer)
    );
    expect(invocation(cancelled!)).toEqual({
      contract: MANAGER,
      method: "cancel_queued_transfer",
      args: [OWNER, 7n],
    });
  });
});

describe("StellarNtt.prepare auth checks", () => {
  const owner = new StellarAddress(OWNER);

  /**
   * Creates a binding whose provider assembles with another copy of
   * stellar-sdk, as one from `@wormhole-foundation/sdk-stellar` can. Each copy
   * has its own XDR classes and enum objects.
   */
  const foreignNtt = async () => {
    let sdk: typeof import("@stellar/stellar-sdk") | undefined;
    await jest.isolateModulesAsync(async () => {
      sdk = await import("@stellar/stellar-sdk");
    });
    expect(sdk!.xdr).not.toBe(xdr);
    const { Transaction: ForeignTransaction, rpc } = sdk!;
    return new StellarNtt(
      "Testnet",
      "Stellar",
      {
        ...provider,
        prepareTransaction: async (tx: Transaction) =>
          rpc
            .assembleTransaction(
              new ForeignTransaction(tx.toXDR(), tx.networkPassphrase),
              simulation(tx)
            )
            .build(),
      } as unknown as SorobanRpc.Server,
      contracts
    );
  };

  it("refuses an auth entry the source account cannot sign", async () => {
    // The manager's owner is `PAUSER`, not the account sourcing the call, so
    // the simulation records address credentials that only it can sign.
    recordAuth = (call) => [
      new xdr.SorobanAuthorizationEntry({
        credentials: xdr.SorobanCredentials.sorobanCredentialsAddress(
          new xdr.SorobanAddressCredentials({
            address: new Address(PAUSER).toScAddress(),
            nonce: xdr.Int64.fromString("1"),
            signatureExpirationLedger: 0,
            signature: xdr.ScVal.scvVoid(),
          })
        ),
        rootInvocation: authorized(call),
      }),
    ];
    for (const n of [ntt(), await foreignNtt()])
      await expect(collect(n.setThreshold(2, owner))).rejects.toThrow(
        `Ntt.setThreshold needs an auth entry signed by ${PAUSER}`
      );
  });

  it("allows no call below an admin, pause or cancel call", async () => {
    const peer = {
      chain: "Ethereum",
      address: new UniversalAddress(new Uint8Array(32).fill(0x11)),
    } as ChainAddress;
    const foreign = await foreignNtt();
    recordAuth = sourceAuth();
    for (const write of [
      ntt().setPeer(peer, 18, 1000n, owner),
      ntt().setThreshold(2, owner),
      ntt().pause(owner),
      ntt().cancelOutboundQueuedTransfer(7n, owner),
      ntt().setTransceiverPeer(0, peer, owner),
      foreign.setPeer(peer, 18, 1000n, owner),
    ])
      await expect(collect(write)).resolves.toHaveLength(1);

    // The owner's signature would authorize this transfer of its tokens too.
    recordAuth = sourceAuth(
      authorized(
        contractCall(
          TOKEN,
          "transfer",
          new Address(OWNER).toScVal(),
          new Address(PAUSER).toScVal(),
          nativeToScVal(1n, { type: "i128" })
        )
      )
    );
    for (const n of [ntt(), foreign])
      await expect(collect(n.setPeer(peer, 18, 1000n, owner))).rejects.toThrow(
        `Unexpected call in the auth tree: ${TOKEN}.transfer`
      );
  });

  it("allows one auth entry, rooted at the call that was built", async () => {
    recordAuth = (call) => [...sourceAuth()(call), ...sourceAuth()(call)];
    await expect(collect(ntt().setThreshold(2, owner))).rejects.toThrow(
      /2 auth entries/
    );

    // An entry for a call this transaction never makes would still be
    // authorized by its signature.
    const other = contractCall(
      MANAGER,
      "set_threshold",
      nativeToScVal(9, { type: "u32" })
    );
    recordAuth = () => sourceAuth()(other);
    await expect(collect(ntt().setThreshold(2, owner))).rejects.toThrow(
      /a call other than the one it makes/
    );
  });
});

describe("StellarNtt transfer auth bounds", () => {
  const XLM = nativeSacId("Testnet");
  const sender = new StellarAddress(OWNER);
  const destination = {
    chain: "Ethereum",
    address: new UniversalAddress(new Uint8Array(32).fill(0x11)),
  } as ChainAddress;
  const send = (n = ntt()) =>
    collect(n.transfer(sender, 42n, destination, { queue: false }));

  /** A `transfer` of `amount` out of `from`. */
  const transferCall = (contract: string, amount: bigint, from = OWNER) =>
    contractCall(
      contract,
      "transfer",
      new Address(from).toScVal(),
      new Address(MANAGER).toScVal(),
      nativeToScVal(amount, { type: "i128" })
    );
  /** The same `transfer`, as the auth tree records it. */
  const move = (contract: string, amount: bigint, from = OWNER) =>
    authorized(transferCall(contract, amount, from));

  // Two enabled transceivers, each paid the core's 100-stroop message fee.
  beforeEach(() => (overrides = { get_enabled_bitmap: 0b101n }));

  it("bounds the token by the amount and XLM by the transceiver fees", async () => {
    recordAuth = sourceAuth(move(TOKEN, 42n), move(XLM, 100n), move(XLM, 100n));
    await expect(send()).resolves.toHaveLength(1);

    recordAuth = sourceAuth(move(TOKEN, 43n));
    await expect(send()).rejects.toThrow(/43 of the token, above the 42/);
    recordAuth = sourceAuth(move(TOKEN, 42n), move(XLM, 201n));
    await expect(send()).rejects.toThrow(/201 stroops of XLM, above the 200/);

    // Burning mode takes custody with `burn(from, amount)`.
    const burn = (amount: bigint) =>
      authorized(
        contractCall(
          TOKEN,
          "burn",
          new Address(OWNER).toScVal(),
          nativeToScVal(amount, { type: "i128" })
        )
      );
    recordAuth = sourceAuth(burn(42n));
    await expect(send()).resolves.toHaveLength(1);
    recordAuth = sourceAuth(burn(43n));
    await expect(send()).rejects.toThrow(/43 of the token, above the 42/);
  });

  it("allows only the token and XLM, moved out of the sender", async () => {
    recordAuth = sourceAuth(move(TRANSCEIVER, 1n));
    await expect(send()).rejects.toThrow(
      `Unexpected call in the auth tree: ${TRANSCEIVER}.transfer`
    );
    recordAuth = sourceAuth(move(TOKEN, 42n, PAUSER));
    await expect(send()).rejects.toThrow(/moves only from/);

    // A negative move would lower the total the bound compares.
    recordAuth = sourceAuth(move(TOKEN, -1n), move(TOKEN, 43n));
    await expect(send()).rejects.toThrow(`The auth tree moves -1 of ${TOKEN}`);
    // A call below a move would escape the bound.
    recordAuth = sourceAuth(
      authorized(transferCall(TOKEN, 42n), move(XLM, 1_000n))
    );
    await expect(send()).rejects.toThrow(
      `Unexpected call in the auth tree: ${TOKEN}.transfer`
    );
    // XLM moves only by `transfer`.
    recordAuth = sourceAuth(
      authorized(
        contractCall(
          XLM,
          "approve",
          new Address(OWNER).toScVal(),
          new Address(MANAGER).toScVal(),
          nativeToScVal(1n, { type: "i128" }),
          nativeToScVal(0, { type: "u32" })
        )
      )
    );
    await expect(send()).rejects.toThrow(
      `Unexpected call in the auth tree: ${XLM}.approve`
    );
  });

  it("adds the amount to the XLM bound when the token is XLM", async () => {
    const xlmNtt = new StellarNtt("Testnet", "Stellar", provider, {
      ...contracts,
      ntt: { ...contracts.ntt, token: XLM },
    });
    recordAuth = sourceAuth(move(XLM, 42n), move(XLM, 200n));
    await expect(send(xlmNtt)).resolves.toHaveLength(1);
    recordAuth = sourceAuth(move(XLM, 43n), move(XLM, 200n));
    await expect(send(xlmNtt)).rejects.toThrow(
      /243 stroops of XLM, above the 242/
    );
  });

  it("bounds a queued completion by the transceiver fees alone", async () => {
    const complete = () =>
      collect(ntt().completeOutboundQueuedTransfer(7n, sender));
    recordAuth = sourceAuth(move(XLM, 200n));
    await expect(complete()).resolves.toHaveLength(1);

    // The tokens left custody when the transfer was queued.
    recordAuth = sourceAuth(move(TOKEN, 1n));
    await expect(complete()).rejects.toThrow(/1 of the token, above the 0/);
  });
});

describe("StellarNtt fee cap", () => {
  const owner = new StellarAddress(OWNER);

  it("refuses a fee above maxFee, and not one at it", async () => {
    // The 100-stroop inclusion fee plus the resource fee comes to exactly 5 XLM.
    resourceFee = 49_999_900;
    await expect(collect(ntt().setThreshold(2, owner))).resolves.toHaveLength(
      1
    );
    resourceFee = 49_999_901;
    await expect(collect(ntt().setThreshold(2, owner))).rejects.toThrow(
      "Ntt.setThreshold costs 50000001 stroops, above maxFee (50000000)"
    );

    const raised = ntt();
    raised.maxFee = 100_000_000n;
    await expect(collect(raised.setThreshold(2, owner))).resolves.toHaveLength(
      1
    );
  });

  it("names auto-restoration when it trips the cap", async () => {
    // The simulation lists the archived entries the write will restore.
    const data = new SorobanDataBuilder().setResourceFee(60_000_000).build();
    data.ext(
      new xdr.SorobanTransactionDataExt(
        1,
        new xdr.SorobanResourcesExtV0({ archivedSorobanEntries: [0] })
      )
    );
    const restoring = new StellarNtt(
      "Testnet",
      "Stellar",
      {
        ...provider,
        prepareTransaction: async (tx: Transaction) =>
          SorobanRpc.assembleTransaction(tx, {
            ...simulation(tx),
            transactionData: data.toXDR("base64"),
          }).build(),
      } as unknown as SorobanRpc.Server,
      contracts
    );
    await expect(collect(restoring.setThreshold(2, owner))).rejects.toThrow(
      /because it restores archived ledger entries\. Restore them in a separate restoreFootprint/
    );
  });

  it("leaves room for the costliest write measured on the localnet", async () => {
    // The resource fee of a first delivery against a 19-guardian set signed
    // to mainnet's quorum of 13.
    resourceFee = 358_889;
    await expect(
      collect(ntt().redeem([attestation], owner))
    ).resolves.toHaveLength(1);
  });
});

describe("StellarNttWormholeTransceiver", () => {
  const owner = new StellarAddress(OWNER);
  const peer = {
    chain: "Ethereum",
    address: new UniversalAddress(new Uint8Array(32).fill(0x11)),
  } as ChainAddress;
  const xcvr = () => new StellarNttWormholeTransceiver(ntt(), TRANSCEIVER);

  it("reports the raw contract id as its emitter, not hash_address", async () => {
    // The same contract has two distinct wire identities. The core takes an
    // emitter's identity from `AddressPayload::ContractIdHash` — the raw
    // contract id — while `hash_address`, the keccak256 of the StrKey text, is
    // what NTT messages carry. A peer registering the hash as this
    // transceiver's emitter would fail every inbound VAA with
    // `TransceiverError::UnexpectedEmitter`, so the two must not be confused.
    const rawId =
      "5c1454589bc12030e0542a8bb7fb7af1e61530959227ac4530ef6d643416e22c";
    const hashAddress =
      "35d8048097dd779df4bee5cf0bd3e871656cf21d77f7701262fab28488577660";
    expect(rawId).not.toEqual(hashAddress);
    expect(
      encoding.hex.encode(
        new StellarAddress(TRANSCEIVER).toUniversalAddress().toUint8Array()
      )
    ).toEqual(hashAddress);

    const address = await xcvr().getAddress();
    expect(address.chain).toEqual("Stellar");
    expect(encoding.hex.encode(address.address.toUint8Array())).toEqual(rawId);
  });

  it("reads its transport identifier off the contract", async () => {
    await expect(xcvr().getTransceiverType()).resolves.toEqual("wormhole");
    expect(calls.map((c) => c.method)).toEqual(["get_transceiver_type"]);
  });

  it("reads a peer's emitter, which stays universal", async () => {
    const emitter = new UniversalAddress(
      new Uint8Array(Buffer.alloc(32, 0xcd))
    );
    await expect(xcvr().getPeer("Ethereum")).resolves.toEqual({
      chain: "Ethereum",
      address: emitter,
    });
    // `get_peer_info` answers "registered?" and "enabled?" in one read; a
    // disabled peer keeps its emitter, so `get_peer` alone cannot tell them
    // apart.
    await expect(xcvr().getPeerInfo("Ethereum")).resolves.toEqual({
      address: { chain: "Ethereum", address: emitter },
      enabled: true,
    });
    await expect(xcvr().isPeerEnabled("Ethereum")).resolves.toEqual(true);
    expect(calls.map((c) => c.args[0]!.u32())).toEqual([2, 2, 2]);
  });

  it("returns null for a chain with no peer at all", async () => {
    overrides = { get_peer: null, get_peer_info: null, is_peer_enabled: false };
    await expect(xcvr().getPeer("Ethereum")).resolves.toBeNull();
    await expect(xcvr().getPeerInfo("Ethereum")).resolves.toBeNull();
    await expect(xcvr().isPeerEnabled("Ethereum")).resolves.toEqual(false);
  });

  it("registers a peer once and disables it separately", async () => {
    expect(await one(xcvr().setPeer(peer, owner))).toEqual({
      contract: TRANSCEIVER,
      method: "set_peer",
      args: [2, Buffer.from(peer.address.toUniversalAddress().toUint8Array())],
    });
    // `set_peer` is one-shot (`PeerAlreadySet`), so this is the only way back.
    expect(await one(xcvr().setPeerEnabled("Ethereum", false, owner))).toEqual({
      contract: TRANSCEIVER,
      method: "set_peer_enabled",
      args: [2, false],
    });
  });

  it("checks replay by the VAA's emitter and sequence, not the digest", async () => {
    await expect(xcvr().isVaaConsumed(attestation)).resolves.toEqual(false);
    expect(calls[0]!.args.map((arg) => scValToNative(arg))).toEqual([
      2,
      Buffer.alloc(32),
      1n,
    ]);
  });

  it("hands a VAA to receive_message, guiding an unregistered recipient", async () => {
    const [tx] = await collect(xcvr().receive(attestation, owner));
    expect(invocation(tx!)).toEqual({
      contract: TRANSCEIVER,
      method: "receive_message",
      args: [Buffer.from(serialize(attestation))],
    });
    // The transaction is a transceiver call, and says so — `redeem` yields
    // this same one, so its description reads `Transceiver.receive` there too,
    // not `Ntt.redeem`.
    expect(tx!.description).toEqual("Transceiver.receive");
    // Permissionless, but Stellar has no implicit signer to fund it.
    await expect(collect(xcvr().receive(attestation))).rejects.toThrow(
      /no implicit signer/
    );
  });

  it("pauses owner-only and has no pauser role to hand out", async () => {
    // Unlike the manager, whose pauser can also pause. The contract ignores
    // the caller it is handed and enforces owner auth, but still takes it.
    expect(await one(xcvr().pause(owner))).toEqual({
      contract: TRANSCEIVER,
      method: "pause",
      args: [OWNER],
    });
    expect(await one(xcvr().unpause(owner))).toEqual({
      contract: TRANSCEIVER,
      method: "unpause",
      args: [OWNER],
    });
    await expect(xcvr().getPauser()).resolves.toBeNull();
    await expect(
      collect(xcvr().setPauser(new StellarAddress(PAUSER), owner))
    ).rejects.toThrow(/owner-only/);
  });
});

describe("StellarNtt.getTransceiver", () => {
  it("has only the Wormhole transceiver, at index 0", async () => {
    await expect(ntt().getTransceiver(1)).resolves.toBeNull();
    await expect(
      new StellarNtt("Testnet", "Stellar", provider, {
        coreBridge: CORE,
        ntt: { ...contracts.ntt, transceiver: {} },
      }).getTransceiver(0)
    ).resolves.toBeNull();
  });

  it("hands back the transceiver binding itself", async () => {
    const transceiver = (await ntt().getTransceiver(0))!;
    expect(transceiver).toBeInstanceOf(StellarNttWormholeTransceiver);
    expect(transceiver.address).toEqual(TRANSCEIVER);
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
