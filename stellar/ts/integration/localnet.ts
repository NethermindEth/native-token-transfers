/**
 * Localnet harness for the integration suite: environment, a funded admin
 * account, contract deployment, and the guardian that signs inbound VAAs.
 *
 * It is the TypeScript counterpart of `stellar/integration-tests/src/{ctx,
 * deploy,vaa}.rs` and reuses that harness's `.env.localnet`, vendored wasms and
 * test guardian verbatim, so both suites run against the same Docker network
 * started by `stellar/integration-tests/scripts/start-localnet.sh`.
 *
 * Every contract call a test makes goes through the package under test, and
 * every transaction through the signer in `@wormhole-foundation/sdk-stellar`;
 * what is built here is what has no binding at all — uploading and
 * instantiating wasm.
 */
import { createECDH, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  Address,
  Asset,
  BASE_FEE,
  Contract,
  Keypair,
  Operation,
  TransactionBuilder,
  nativeToScVal,
  rpc as SorobanRpc,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import type { Transaction } from "@stellar/stellar-sdk";
import type { Chain } from "@wormhole-foundation/sdk-base";
import {
  UniversalAddress,
  createVAA,
  keccak256,
} from "@wormhole-foundation/sdk-definitions";
import type { VAA } from "@wormhole-foundation/sdk-definitions";
import { mocks } from "@wormhole-foundation/sdk-definitions/testing";
import type { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import {
  StellarAddress,
  StellarPlatform,
  StellarUnsignedTransaction,
  getStellarSigner,
  nativeSacId,
  stellarNetworkPassphrase,
} from "@wormhole-foundation/sdk-stellar";
import { StellarNtt } from "../src/ntt.js";
import { StellarNttWithExecutor } from "../src/nttWithExecutor.js";
import { bytesArg } from "../src/scval-types.js";

/** sdk-base registers the localnet passphrase as Stellar's `Devnet`. */
export const NETWORK = "Devnet";
export const CHAIN = "Stellar";

/** `stellar/` — the Rust harness's root, which its wasm paths are relative to. */
const STELLAR_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

const env = loadEnv();

export const rpc = new SorobanRpc.Server(env.rpcUrl, { allowHttp: true });

/**
 * The account every transaction in a test file is sourced from. Regenerated per
 * run because the container is `--rm` and keeps no state; per *file*, because
 * the module registry is, which is one less thing for the single worker to
 * serialize.
 */
export const admin = Keypair.random();

/** The guardian set the deployed core is constructed with, at index 0. */
const guardians = new mocks.MockGuardians(0, [env.guardianSecretHex]);

/**
 * Reads `.env.localnet` — the same file `scripts/run-tests.sh` sources — and
 * absolutizes the wasm paths it holds relative to `stellar/`, as that script
 * does. A missing key fails here rather than as an unreadable file later.
 *
 * Parses rather than hands the file to `process.loadEnvFile`, which writes to
 * the real process environment while a Jest module sees a sandboxed copy of it.
 */
function loadEnv() {
  const file = resolve(STELLAR_DIR, "integration-tests/.env.localnet");
  const values = new Map(
    readFileSync(file, "utf8")
      .split("\n")
      .flatMap((line) => {
        const [, key, value] = /^\s*(\w+)\s*=\s*"?(.*?)"?\s*$/.exec(line) ?? [];
        return key === undefined || value === undefined ? [] : [[key, value]];
      })
  );
  const read = (key: string): string => {
    const value = values.get(key);
    if (!value) throw new Error(`${key} is not set in ${file}`);
    return value;
  };
  const wasm = (key: string): string => {
    const path = read(key);
    return isAbsolute(path) ? path : resolve(STELLAR_DIR, path);
  };
  return {
    rpcUrl: read("SOROBAN_RPC_URL"),
    friendbotUrl: read("STELLAR_FRIENDBOT_URL"),
    chainId: Number(read("STELLAR_CHAIN_ID")),
    guardianSecretHex: read("GUARDIAN_SECRET_HEX"),
    managerWasm: wasm("NTT_MANAGER_WASM_PATH"),
    transceiverWasm: wasm("NTT_TRANSCEIVER_WASM_PATH"),
    wrapperWasm: wasm("NTT_WITH_EXECUTOR_WASM_PATH"),
    mockTokenWasm: wasm("MOCK_TOKEN_WASM_PATH"),
    coreWasm: wasm("WORMHOLE_CORE_WASM_PATH"),
    executorWasm: wasm("WORMHOLE_EXECUTOR_WASM_PATH"),
  };
}

/**
 * Funds `account` through friendbot and blocks until Soroban RPC serves it, so the
 * caller can source a transaction from it immediately: friendbot answers before
 * the funding ledger closes, hence the poll rather than a bare request.
 */
export async function fund(account: Keypair): Promise<Keypair> {
  // A non-2xx is not yet a failure: friendbot refuses an account it has already
  // funded, which is exactly the state the caller wants. It only matters if the
  // account never shows up, so it is reported there.
  const funding = await fetch(
    `${env.friendbotUrl}?addr=${account.publicKey()}`
  );
  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      await rpc.getAccount(account.publicKey());
      return account;
    } catch {
      await sleep(1000);
    }
  }
  throw new Error(
    `friendbot answered ${funding.status} and ${account.publicKey()} never appeared`
  );
}

/**
 * Builds, simulates, assembles, and submits `operation` from the admin account,
 * returning the value the invocation produced. The same build → simulate →
 * assemble path `StellarNtt.prepare` takes, so a deployment exercises it too.
 */
async function send(
  operation: xdr.Operation,
  description: string
): Promise<unknown> {
  const source = await rpc.getAccount(admin.publicKey());
  const tx = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: stellarNetworkPassphrase(NETWORK),
  })
    .addOperation(operation)
    .setTimeout(30)
    .build();

  const simulation = await simulate(tx, description);
  return submit(
    new StellarUnsignedTransaction(
      SorobanRpc.assembleTransaction(tx, simulation).build(),
      NETWORK,
      CHAIN,
      description
    )
  );
}

/**
 * Drives a binding's generator to completion, returning what each call
 * returned — a manager `transfer` reports the sequence it was assigned.
 */
export async function drive(
  txs: AsyncGenerator<StellarUnsignedTransaction<typeof NETWORK, typeof CHAIN>>
): Promise<unknown[]> {
  const results: unknown[] = [];
  for await (const tx of txs) results.push(await submit(tx));
  return results;
}

async function simulate(
  transaction: Transaction,
  description: string
): Promise<SorobanRpc.Api.SimulateTransactionSuccessResponse> {
  const simulation = await rpc.simulateTransaction(transaction);
  if (SorobanRpc.Api.isSimulationError(simulation))
    throw new Error(`${description} would fail: ${simulation.error}`);
  return simulation;
}

/**
 * Signs, submits, and confirms one assembled transaction as {@link admin}
 * through the `StellarSigner` and `StellarPlatform.sendAndConfirm` of
 * `@wormhole-foundation/sdk-stellar`, the path a consumer takes, and returns
 * what the invocation returned.
 *
 * The signer reports only the hash, so the applied return value is read back
 * from the confirmed transaction.
 */
async function submit(
  tx: StellarUnsignedTransaction<typeof NETWORK, typeof CHAIN>
): Promise<unknown> {
  const signer = await getStellarSigner(rpc, admin.secret());
  const [hash] = await signer.signAndSend([tx]);
  if (hash === undefined) throw new Error(`${tx.description} returned no hash`);
  const result = await rpc.getTransaction(hash);
  if (result.status !== "SUCCESS")
    throw new Error(`${tx.description} reads back as ${result.status}`);
  return result.returnValue ? scValToNative(result.returnValue) : undefined;
}

/**
 * Uploads `wasmPath` and instantiates it with `constructorArgs`, returning the
 * new contract id. A fresh salt per call means one wasm can back several
 * independent instances.
 */
async function deploy(
  wasmPath: string,
  ...constructorArgs: xdr.ScVal[]
): Promise<string> {
  const wasmHash = await send(
    Operation.uploadContractWasm({ wasm: readFileSync(wasmPath) }),
    `upload ${wasmPath}`
  );
  if (!(wasmHash instanceof Uint8Array))
    throw new Error(`Upload of ${wasmPath} returned no wasm hash`);

  const contract = await send(
    Operation.createCustomContract({
      address: new Address(admin.publicKey()),
      wasmHash,
      salt: randomBytes(32),
      constructorArgs,
    }),
    `deploy ${wasmPath}`
  );
  if (typeof contract !== "string")
    throw new Error(`Deploy of ${wasmPath} returned no contract id`);
  return contract;
}

/**
 * Deploys the native XLM Stellar Asset Contract on first use and returns its
 * id. The id derives from the network passphrase, so it is the same address on
 * every call and a second deploy would fail — hence the check rather than a
 * swallowed error, which would also hide an RPC that is merely down.
 */
export async function nativeSac(): Promise<string> {
  const id = nativeSacId(NETWORK);
  if (!(await isDeployed(id)))
    await send(
      Operation.createStellarAssetContract({ asset: Asset.native() }),
      "deploy native SAC"
    );
  return id;
}

/** Checks whether a contract instance exists at `contractId`. */
async function isDeployed(contractId: string): Promise<boolean> {
  const { entries } = await rpc.getLedgerEntries(
    new Contract(contractId).getFootprint()
  );
  return entries.length > 0;
}

type StackOptions = {
  mode: Ntt.Mode;
  /** Ignored in locking mode, which uses the 7-decimal native SAC. */
  tokenDecimals: number;
  /**
   * In the *trimmed* domain, which is what the constructor takes directly —
   * unlike `StellarNtt.setOutboundLimit`, which trims for you. The two are the
   * same number for a 7-decimal token, whose trimmed domain is also 7.
   */
  outboundLimit: bigint;
  /** Seconds — Soroban ledger time, not milliseconds. */
  rateLimitDuration: bigint;
};

/** One deployed NTT stack, with the bindings under test already built on it. */
export type Stack = {
  core: string;
  token: string;
  manager: string;
  transceiver: string;
  ntt: StellarNtt<typeof NETWORK, typeof CHAIN>;
};

/**
 * Deploys a core holding the test guardian, a token, a manager, and one
 * transceiver, then registers the transceiver — which also raises the threshold
 * from 0 to 1. The caller registers the peers, since half the tests are about
 * peers.
 */
export async function deployStack(
  options: Partial<StackOptions> = {}
): Promise<Stack> {
  const {
    mode = "burning",
    tokenDecimals = 7,
    outboundLimit = MAX_U64,
    rateLimitDuration = 1n,
  } = options;

  const core = await deploy(
    env.coreWasm,
    xdr.ScVal.scvVec([bytesArg(guardianAddress())]),
    bytesArg(GOVERNANCE_EMITTER)
  );
  const token =
    mode === "locking"
      ? await nativeSac()
      : await deploy(
          env.mockTokenWasm,
          nativeToScVal(tokenDecimals, { type: "u32" })
        );
  const manager = await deploy(
    env.managerWasm,
    addressArg(admin.publicKey()),
    addressArg(token),
    // `Mode` is a unit enum, which crosses the ABI as its u32 discriminant.
    nativeToScVal(mode === "locking" ? 0 : 1, { type: "u32" }),
    nativeToScVal(env.chainId, { type: "u32" }),
    nativeToScVal(outboundLimit, { type: "u64" }),
    nativeToScVal(rateLimitDuration, { type: "u64" }),
    addressArg(core)
  );
  const transceiver = await deploy(
    env.transceiverWasm,
    addressArg(admin.publicKey()),
    addressArg(manager),
    addressArg(core)
  );

  const ntt = new StellarNtt(NETWORK, CHAIN, rpc, {
    coreBridge: core,
    ntt: { manager, token, transceiver: { wormhole: transceiver } },
  });
  await drive(ntt.setTransceiver(transceiver, adminAddress()));

  return { core, token, manager, transceiver, ntt };
}

/**
 * Deploys the `ntt-with-executor` wrapper against a freshly deployed Executor
 * rail. The wrapper binds its executor at construction, so the two are one unit;
 * the binding takes the wrapper address as its constructor override and the
 * executor in its contracts, since `STELLAR_ADDRESSES` has no localnet entry to
 * look up.
 */
export async function deployExecutor(
  ntt: StellarNtt<typeof NETWORK, typeof CHAIN>
): Promise<{
  executor: string;
  withExecutor: StellarNttWithExecutor<typeof NETWORK, typeof CHAIN>;
}> {
  const executor = await deploy(
    env.executorWasm,
    nativeToScVal(env.chainId, { type: "u32" }),
    addressArg(await nativeSac())
  );
  const wrapper = await deploy(env.wrapperWasm, addressArg(executor));
  return {
    executor,
    withExecutor: new StellarNttWithExecutor(
      NETWORK,
      CHAIN,
      rpc,
      { ...ntt.contracts, executor },
      wrapper
    ),
  };
}

export const adminAddress = (): StellarAddress =>
  new StellarAddress(admin.publicKey());

/** Returns `hash_address`, the identity NTT messages carry for a Stellar address. */
export const hashAddress = (address: string): Uint8Array =>
  new StellarAddress(address).toUniversalAddress().toUint8Array();

/** Credits `amount` on the mock token, the one contract with no binding. */
export async function mint(
  token: string,
  to: string,
  amount: bigint
): Promise<void> {
  await send(
    new Contract(token).call(
      "mint",
      addressArg(to),
      nativeToScVal(amount, { type: "i128" })
    ),
    "mint"
  );
}

/** Gets the balance of `holder` on a SEP-41 token contract, in the token's decimals. */
export async function balanceOf(
  token: string,
  holder: string
): Promise<bigint> {
  const balance = await StellarPlatform.getBalance(
    NETWORK,
    CHAIN,
    rpc,
    holder,
    token
  );
  return balance ?? 0n;
}

/**
 * Creates a guardian-signed VAA carrying `nttManagerPayload` from `emitter`, in
 * the `Ntt:WormholeTransfer` shape the Wormhole transceiver expects.
 */
export function inboundVaa(args: {
  emitterChain: Chain;
  emitter: Uint8Array;
  sequence: bigint;
  sourceNttManager: Uint8Array;
  recipientNttManager: Uint8Array;
  nttManagerPayload: Ntt.Message;
}): VAA<"Ntt:WormholeTransfer"> {
  const signed = guardians.addSignatures(
    createVAA("Ntt:WormholeTransfer", {
      guardianSet: 0,
      signatures: [],
      timestamp: 0,
      nonce: 0,
      emitterChain: args.emitterChain,
      emitterAddress: new UniversalAddress(args.emitter),
      sequence: args.sequence,
      consistencyLevel: 1,
      payload: {
        sourceNttManager: new UniversalAddress(args.sourceNttManager),
        recipientNttManager: new UniversalAddress(args.recipientNttManager),
        nttManagerPayload: args.nttManagerPayload,
      },
    })
  );
  // `addSignatures` also accepts raw bytes, so its return type is a union.
  if (signed.payloadLiteral !== "Ntt:WormholeTransfer")
    throw new Error(`Signed the wrong payload: ${signed.payloadLiteral}`);
  return signed;
}

/**
 * Polls for the most recent contract event named `name`, because RPC indexing
 * lags ledger close by a few seconds and a single fetch right after a
 * transaction usually misses it. Returns its data map, keyed by the Rust field
 * names. Most recent, not first: a contract that emits the same event twice in
 * a run would otherwise keep answering with the first one.
 */
export async function findEvent(
  contractId: string,
  name: string
): Promise<Record<string, unknown>> {
  const startLedger = Math.max(1, (await rpc.getLatestLedger()).sequence - 200);
  const deadline = Date.now() + EVENT_TIMEOUT_MS;
  do {
    const { events } = await rpc.getEvents({
      startLedger,
      filters: [{ type: "contract", contractIds: [contractId] }],
    });
    const event = events.findLast((e) =>
      e.topic.some((topic) => scValToNative(topic) === name)
    );
    if (event) {
      const data: unknown = scValToNative(event.value);
      if (!isMap(data))
        throw new Error(`${name} event data is not a map: ${String(data)}`);
      return data;
    }
    await sleep(1000);
  } while (Date.now() < deadline);
  throw new Error(
    `No ${name} event from ${contractId} in ${EVENT_TIMEOUT_MS}ms`
  );
}

const isMap = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Generous: it only bounds the failure path, since a present event returns at once. */
const EVENT_TIMEOUT_MS = 30_000;

/**
 * Retries `attempt` until the rate limiter releases the transfer it is holding
 * at `releaseTimestamp`.
 *
 * The gate is `now >= release_timestamp` on the **ledger** clock, which advances
 * only as ledgers close, so the budget is taken from the queue entry rather than
 * guessed in wall-clock seconds: a slow machine then waits longer instead of
 * failing, and a transfer still held once ledger time is past its own gate is a
 * real failure. So is any error other than `TransferNotReleasable (64)`, which
 * the loop rethrows immediately.
 */
export async function completeAfterWindow(
  attempt: () => Promise<unknown>,
  releaseTimestamp: bigint
): Promise<void> {
  for (;;) {
    try {
      await attempt();
      return;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (!message.includes("TransferNotReleasable")) throw error;

      const now = await ledgerTime();
      if (now > releaseTimestamp + RELEASE_GRACE_SECONDS)
        throw new Error(
          `Ledger time ${now} is past the ${releaseTimestamp} the transfer ` +
            `was queued until, and it is still held: ${message}`
        );
      await sleep(2000);
    }
  }
}

/**
 * Gets the ledger close time in seconds, which is the clock the release gate
 * reads. `Api.GetLatestLedgerResponse` does not declare `closeTime`, but the
 * client returns the RPC's whole answer, so the field is narrowed here.
 */
export async function ledgerTime(): Promise<bigint> {
  const ledger = await rpc.getLatestLedger();
  const closeTime = "closeTime" in ledger ? ledger.closeTime : undefined;
  if (typeof closeTime !== "string" && typeof closeTime !== "number")
    throw new Error(`getLatestLedger returned no closeTime`);
  return BigInt(closeTime);
}

/** Room for the ledger to close after the gate opens before that is a failure. */
const RELEASE_GRACE_SECONDS = 30n;

/**
 * Returns the 20-byte Ethereum address of the test guardian, which is what the
 * core stores and recovers signatures against. sdk-definitions' `SignatureUtils`
 * hands back the *compressed* point, so the uncompressed one comes from Node's
 * own ECDH rather than from a new dependency.
 */
function guardianAddress(): Uint8Array {
  const ecdh = createECDH("secp256k1");
  ecdh.setPrivateKey(Buffer.from(env.guardianSecretHex, "hex"));
  return keccak256(ecdh.getPublicKey().subarray(1)).subarray(12);
}

const addressArg = (address: string): xdr.ScVal =>
  new Address(address).toScVal();

const sleep = (ms: number): Promise<void> =>
  new Promise((done) => setTimeout(done, ms));

/** `wormhole_soroban_client::GOVERNANCE_EMITTER` — `0x00…04`, fixed at deploy. */
const GOVERNANCE_EMITTER = Uint8Array.from({ length: 32 }, (_, i) =>
  i === 31 ? 4 : 0
);

/** The manager stores rate limits as a bare `u64`, so this is "no limit". */
const MAX_U64 = 2n ** 64n - 1n;
