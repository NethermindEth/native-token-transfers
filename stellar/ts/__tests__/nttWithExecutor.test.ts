// ESM Jest does not inject the `jest` global; import it.
import { jest } from "@jest/globals";
import {
  Account,
  Address,
  Operation,
  scValToNative,
  type Transaction,
  type rpc as SorobanRpc,
  type xdr,
} from "@stellar/stellar-sdk";
import { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import type { AccountAddress } from "@wormhole-foundation/sdk-definitions";
import type { NttWithExecutor } from "@wormhole-foundation/sdk-definitions-ntt";
import { StellarAddress, nativeSacId } from "@wormhole-foundation/sdk-stellar";
import type { StellarUnsignedTransaction } from "@wormhole-foundation/sdk-stellar";
import { StellarNtt } from "../src/ntt.js";
import type { AuthorizedCall } from "../src/ntt.js";
import { StellarNttWithExecutor } from "../src/nttWithExecutor.js";

const MANAGER = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
const TOKEN = "CBD3J5AK3ZNX3CJVELGW2N32P3CI2TLMRIQEIL5OXKWA2HH2LN2DA3AO";
const TRANSCEIVER = "CBOBIVCYTPASAMHAKQVIXN73PLY6MFJQSWJCPLCFGDXW2ZBUC3RCZN5S";
const CORE = "CDKTO4XRUS3SHEFSYEAAGVR5MCWIYDR5EFUVH5AUZXX5MAEM4KB5BSED";
const WRAPPER = "CAIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRCEIRDB3V";
const EXECUTOR = "CDXO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XO4M7R";
const SENDER = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";
const REFERRER = "GC4E6RZNOREPKUIGQR3BLPNGPUT3MAPLZV4LGF7UTLR2G5JKHKVYQVHZ";

// The payee the quoter signs is 32 raw bytes, which are a valid ed25519 public
// key *and* a valid contract id. These are the two readings of the same bytes.
const PAYEE_BYTES = new Uint8Array(32).fill(0xaa);
const PAYEE_ACCOUNT =
  "GCVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVH7N";
const PAYEE_CONTRACT =
  "CCVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKVKUD2U";

const contracts = {
  coreBridge: CORE,
  executor: EXECUTOR,
  ntt: {
    manager: MANAGER,
    token: TOKEN,
    transceiver: { wormhole: TRANSCEIVER },
  },
};

/** Whether a contract instance is deployed at the payee's bytes. */
let instances: unknown[];
/** The ledger keys the payee lookup asked about. */
let probed: xdr.LedgerKey[];
/** What `prepareTransaction` rejects with, if the simulation is to fail. */
let simulationError: Error | undefined;

// The wrapper reads no contract state, and this simulation records no auth
// entries for the policy to check, so a write only needs the rpc calls
// `prepare` and the payee lookup make; `prepareTransaction` hands the built
// transaction straight back so the test can inspect it.
const provider = {
  getAccount: async (address: string) => new Account(address, "17"),
  prepareTransaction: async (tx: Transaction) => {
    if (simulationError) throw simulationError;
    return tx;
  },
  getLedgerEntries: async (...keys: xdr.LedgerKey[]) => {
    probed.push(...keys);
    return { entries: instances };
  },
} as unknown as SorobanRpc.Server;

beforeEach(() => {
  instances = [];
  probed = [];
  simulationError = undefined;
});

const ntt = () => new StellarNtt("Testnet", "Stellar", provider, contracts);

const executor = (wrapper?: string) =>
  new StellarNttWithExecutor(
    "Testnet",
    "Stellar",
    provider,
    contracts,
    wrapper
  );

const AMOUNT = 1_000_000n;

// `dbps` 500 over a 100_000 denominator is 0.5%: the wrapper takes 5_000 of the
// amount above and bridges 995_000. Both numbers are the route's preview of the
// split the contract makes itself.
const quote = (): NttWithExecutor.Quote => ({
  signedQuote: new Uint8Array([1, 2, 3, 4]),
  relayInstructions: new Uint8Array([5, 6, 7, 8]),
  estimatedCost: 2_500_000n,
  payeeAddress: PAYEE_BYTES,
  referrer: { chain: "Stellar", address: new StellarAddress(REFERRER) },
  referrerFee: 5_000n,
  remainingAmount: 995_000n,
  referrerFeeDbps: 500n,
  expires: new Date(Date.now() + 3_600_000),
  gasDropOff: 0n,
});

const destination = {
  chain: "Ethereum",
  address: new UniversalAddress(new Uint8Array(32).fill(0x22)),
} as const;

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

const build = async (q: NttWithExecutor.Quote = quote()) => {
  const txs = executor(WRAPPER).transfer(
    SENDER as unknown as AccountAddress<"Stellar">,
    destination,
    AMOUNT,
    q,
    ntt()
  );
  return (await txs.next()).value!;
};

const transfer = async (q?: NttWithExecutor.Quote) =>
  invocation(await build(q));

describe("StellarNttWithExecutor transfer", () => {
  it("invokes the wrapper with the manager, destination, fee and executor args", async () => {
    const tx = await build();
    const { contract, method, args } = invocation(tx);

    expect({ contract, method, description: tx.description }).toEqual({
      contract: WRAPPER,
      method: "transfer",
      description: "NttWithExecutor.transfer",
    });
    expect(args).toEqual([
      SENDER,
      MANAGER,
      AMOUNT,
      { chain: 2, recipient: Buffer.alloc(32, 0x22) },
      { dbps: 500, referrer: REFERRER },
      {
        amount: 2_500_000n,
        payee: PAYEE_ACCOUNT,
        refund: SENDER,
        relay_instructions: Buffer.from([5, 6, 7, 8]),
        signed_quote: Buffer.from([1, 2, 3, 4]),
      },
    ]);
  });

  it("sends the whole amount and leaves the referrer split to the contract", async () => {
    // The wrapper derives the fee from `dbps` and bridges the remainder, so
    // passing `quote.remainingAmount` (995_000) here would take the fee twice.
    const { args } = await transfer();
    expect(args[2]).toEqual(AMOUNT);
    expect(args[4]).toEqual({ dbps: 500, referrer: REFERRER });
  });

  it("resolves the quote payee against what is deployed at its bytes", async () => {
    // Both encodings satisfy the executor's payee check — it accepts either
    // address payload — so only a deployed instance tells the two apart.
    expect((await transfer()).args[5].payee).toEqual(PAYEE_ACCOUNT);

    instances = [{ key: "contract instance" }];
    expect((await transfer()).args[5].payee).toEqual(PAYEE_CONTRACT);

    // The instance asked about has to be the payee's own, or the answer is
    // about some other contract entirely.
    const data = probed[0]!.contractData();
    expect({
      contract: Address.fromScAddress(data.contract()).toString(),
      key: data.key().switch().name,
      durability: data.durability().name,
    }).toEqual({
      contract: PAYEE_CONTRACT,
      key: "scvLedgerKeyContractInstance",
      durability: "persistent",
    });
  });

  it("rejects a payee that is not 32 bytes", async () => {
    await expect(
      transfer({ ...quote(), payeeAddress: new Uint8Array(20) })
    ).rejects.toThrow(/32-byte quote payee/);
  });

  it("rejects an expired quote before building anything", async () => {
    await expect(
      transfer({ ...quote(), expires: new Date(Date.now() - 1) })
    ).rejects.toThrow(/Quote expired/);
  });

  it("rejects a referrer this chain cannot pay", async () => {
    const referrer = {
      chain: "Ethereum",
      address: new UniversalAddress(new Uint8Array(32).fill(0x33)),
    } as const;
    await expect(transfer({ ...quote(), referrer })).rejects.toThrow(
      /native Stellar address/
    );
  });

  it("rejects a referrer fee that does not fit the u16 wire field", async () => {
    // u16::MAX is the boundary the contract itself still accepts (fee.rs), so
    // the check has to reject above it and not at it.
    const dbps = async (referrerFeeDbps: bigint) =>
      (await transfer({ ...quote(), referrerFeeDbps })).args[4].dbps;
    await expect(dbps(65_535n)).resolves.toEqual(65_535);

    await expect(
      transfer({ ...quote(), referrerFeeDbps: 65_536n })
    ).rejects.toThrow(/does not fit the u16/);
    await expect(
      transfer({ ...quote(), referrerFeeDbps: -1n })
    ).rejects.toThrow(/does not fit the u16/);
  });

  it("names a rejection against the wrapper's own error space", async () => {
    // Against the manager's space the same code reads `NttManagerError::…`, so
    // the prefix is what pins the error space this call passes to `prepare`.
    simulationError = new Error("HostError: Error(Contract, #62)");
    await expect(transfer()).rejects.toThrow(
      /NttWithExecutorError::TransferExceedsRateLimit \(62\)/
    );
  });

  it("refuses to guess an address for an undeployed wrapper", async () => {
    // `STELLAR_ADDRESSES` carries no Stellar deployment, so there is nothing
    // to fall back to and inventing one would be worse than failing.
    const txs = executor().transfer(
      SENDER as unknown as AccountAddress<"Stellar">,
      destination,
      AMOUNT,
      quote(),
      ntt()
    );
    await expect(txs.next()).rejects.toThrow(/No ntt-with-executor/);
  });

  it("refuses to guess the executor the wrapper was built with", async () => {
    // The table has no entry either, and the executor is resolved before the
    // payee lookup, so nothing reaches the rpc.
    const { executor: _, ...unconfigured } = contracts;
    const txs = new StellarNttWithExecutor(
      "Testnet",
      "Stellar",
      provider,
      unconfigured,
      WRAPPER
    ).transfer(
      SENDER as unknown as AccountAddress<"Stellar">,
      destination,
      AMOUNT,
      quote(),
      ntt()
    );
    await expect(txs.next()).rejects.toThrow(
      "No executor configured for Stellar on Testnet"
    );
    expect(probed).toEqual([]);
  });
});

describe("StellarNttWithExecutor transfer auth bounds", () => {
  const XLM = nativeSacId("Testnet");
  const OTHER = "CB3XO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XO53XOJMC";

  /** A `transfer` of `amount` out of `from`, as the policy sees it. */
  const move = (
    contract: string,
    to: string,
    amount: bigint,
    from = SENDER
  ): AuthorizedCall => ({
    contract,
    method: "transfer",
    args: [from, to, amount],
    children: [],
  });

  /**
   * The manager transfer the wrapper makes: `sender`, `amount`,
   * `recipient_chain`, `recipient`, `should_queue`, with its custody and
   * transceiver fee below it.
   */
  const managerTransfer = (
    bridged: bigint,
    args: unknown[] = [SENDER, bridged, 2, Buffer.alloc(32, 0x22), false]
  ): AuthorizedCall => ({
    contract: MANAGER,
    method: "transfer",
    args,
    children: [move(TOKEN, MANAGER, bridged), move(XLM, TRANSCEIVER, 100n)],
  });

  /** The executor's `request_execution`, paying `fees` below it. */
  const execution = (...fees: AuthorizedCall[]): AuthorizedCall => ({
    contract: EXECUTOR,
    method: "request_execution",
    args: [],
    children: fees,
  });

  const referrerFee = move(TOKEN, REFERRER, 5_000n);
  const relayFee = move(XLM, PAYEE_ACCOUNT, 2_500_000n);

  /**
   * What the sender authorizes below the wrapper call: the referrer fee, the
   * manager transfer, and the executor's relay fee.
   */
  const tree = (): AuthorizedCall[] => [
    referrerFee,
    managerTransfer(995_000n),
    execution(relayFee),
  ];

  /** The policy `transfer` hands to `prepare`. */
  const policy = async () => {
    const manager = ntt();
    const prepare = jest.spyOn(manager, "prepare");
    await executor(WRAPPER)
      .transfer(
        SENDER as unknown as AccountAddress<"Stellar">,
        destination,
        AMOUNT,
        quote(),
        manager
      )
      .next();
    return prepare.mock.calls[0]![4]!;
  };

  it("allows the referrer fee, the manager transfer and the relay fee", async () => {
    await expect((await policy())(tree())).resolves.toBeUndefined();
    // A tree with no referrer fee, as at zero dbps, bridges the whole amount;
    // the quote's dbps only bound the fee from above.
    await expect(
      (await policy())([managerTransfer(AMOUNT), execution(relayFee)])
    ).resolves.toBeUndefined();
  });

  it("checks every argument of the manager transfer", async () => {
    const check = await policy();
    const args = managerTransfer(995_000n).args;
    // Another sender, chain or recipient, or a queued transfer.
    for (const [index, value] of [
      [0, REFERRER],
      [2, 3],
      [3, Buffer.alloc(32, 0x33)],
      [4, true],
    ] as const) {
      const changed = [...args];
      changed[index] = value;
      await expect(
        check([
          referrerFee,
          managerTransfer(995_000n, changed),
          execution(relayFee),
        ])
      ).rejects.toThrow(/manager transfer is not the one this call requests/);
    }
  });

  it("requires exactly one manager transfer", async () => {
    const check = await policy();
    await expect(check([referrerFee, execution(relayFee)])).rejects.toThrow(
      "The auth tree holds no manager transfer"
    );
    await expect(
      check([
        move(TOKEN, REFERRER, 0n),
        managerTransfer(500_000n),
        managerTransfer(500_000n),
        execution(relayFee),
      ])
    ).rejects.toThrow("The auth tree holds a second manager transfer");
  });

  it("splits the amount between the referrer and the bridge", async () => {
    const check = await policy();
    await expect(
      check([
        move(TOKEN, REFERRER, 5_001n),
        managerTransfer(995_000n),
        execution(relayFee),
      ])
    ).rejects.toThrow(
      "The auth tree bridges 995000 and pays the referrer 5001, not the 1000000 sent"
    );
    // The quote's 500 dbps of 1_000_000 allow a referrer fee of 5_000 at most.
    await expect(
      check([
        move(TOKEN, REFERRER, 999_999n),
        managerTransfer(1n),
        execution(relayFee),
      ])
    ).rejects.toThrow(
      "The auth tree pays the referrer 999999, above 500 dbps of the 1000000 sent"
    );
    // The manager's custody is bounded by what it bridges, not by `amount`.
    await expect(
      check([
        referrerFee,
        {
          ...managerTransfer(995_000n),
          children: [move(TOKEN, MANAGER, AMOUNT)],
        },
        execution(relayFee),
      ])
    ).rejects.toThrow(/1000000 of the token, above the 995000/);
    // A negative referrer fee would let the bridged amount exceed `amount`.
    await expect(
      check([
        move(TOKEN, REFERRER, -1n),
        managerTransfer(AMOUNT + 1n),
        execution(relayFee),
      ])
    ).rejects.toThrow(`The auth tree moves -1 of ${TOKEN}`);
  });

  it("pays the referrer and the payee only", async () => {
    const check = await policy();
    await expect(
      check([
        move(TOKEN, OTHER, 5_000n),
        managerTransfer(995_000n),
        execution(relayFee),
      ])
    ).rejects.toThrow(
      `The auth tree moves ${TOKEN} from ${SENDER} to ${OTHER}`
    );
    await expect(
      check([
        referrerFee,
        managerTransfer(995_000n),
        execution(move(XLM, OTHER, 2_500_000n)),
      ])
    ).rejects.toThrow(`The auth tree moves ${XLM} from ${SENDER} to ${OTHER}`);
  });

  it("bounds the relay fee by the quote and pays it only in XLM", async () => {
    const check = await policy();
    await expect(
      check([
        referrerFee,
        managerTransfer(995_000n),
        execution(move(XLM, PAYEE_ACCOUNT, 2_500_001n)),
      ])
    ).rejects.toThrow(
      "The auth tree pays the executor 2500001 stroops, above the 2500000 quoted"
    );
    await expect(
      check([
        referrerFee,
        managerTransfer(995_000n),
        execution(move(TOKEN, PAYEE_ACCOUNT, 1n)),
      ])
    ).rejects.toThrow(`Unexpected call in the auth tree: ${TOKEN}.transfer`);
  });

  it("calls only the executor the wrapper was built with", async () => {
    const check = await policy();
    await expect(
      check([
        referrerFee,
        managerTransfer(995_000n),
        { ...execution(relayFee), contract: OTHER },
      ])
    ).rejects.toThrow(
      `Unexpected call in the auth tree: ${OTHER}.request_execution`
    );
    // XLM moved by the wrapper itself rather than by the executor.
    await expect(check([...tree(), move(XLM, REFERRER, 1n)])).rejects.toThrow(
      `Unexpected call in the auth tree: ${XLM}.transfer`
    );
  });
});

describe("StellarNttWithExecutor estimateMsgValueAndGasLimit", () => {
  it("reports that Soroban has neither knob, with no wrapper deployed", async () => {
    // The route asks this of the *destination* chain, which needs no wrapper of
    // its own; requiring one would fail every executor quote into Stellar.
    await expect(
      executor().estimateMsgValueAndGasLimit(undefined)
    ).resolves.toEqual({ msgValue: 0n, gasLimit: 0n });
  });
});
