import {
  Address,
  BASE_FEE,
  Contract,
  TransactionBuilder,
  nativeToScVal,
  rpc as SorobanRpc,
  xdr,
} from "@stellar/stellar-sdk";
import { toChainId } from "@wormhole-foundation/sdk-base";
import type { Chain, Network } from "@wormhole-foundation/sdk-base";
import { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import type {
  AccountAddress,
  ChainAddress,
  ChainsConfig,
  Contracts,
} from "@wormhole-foundation/sdk-definitions";
import { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import {
  StellarAddress,
  StellarPlatform,
  StellarUnsignedTransaction,
  stellarNetworkPassphrase,
} from "@wormhole-foundation/sdk-stellar";
import type {
  AnyStellarAddress,
  StellarChains,
  StellarPlatformType,
} from "@wormhole-foundation/sdk-stellar";
import { decodeContractError } from "./errors.js";
import type { ContractErrorSpace } from "./errors.js";
import {
  asBigint,
  asBoolean,
  asNumber,
  asAddress,
  decodeInboundQueuedTransfer,
  decodeMode,
  decodeNttManagerPeer,
  decodeOutboundQueuedTransfer,
  decodeRateLimitParams,
  decodeTransceiverFee,
  decodeTransceiverInfo,
} from "./scval-types.js";
import type { OutboundQueuedTransfer } from "./scval-types.js";

/**
 * NTT bindings for the Soroban manager + Wormhole transceiver contracts.
 *
 * Every read is a read-only `simulateTransaction` against the manager
 * (`StellarPlatform.simulateRead`), and every write yields a prepared,
 * non-parallelizable `StellarUnsignedTransaction` for the signer to submit.
 */
export class StellarNtt<N extends Network, C extends StellarChains> {
  readonly managerAddress: string;
  readonly tokenAddress: string;
  /**
   * The Wormhole transceiver, if the config names one; Stellar registers no
   * other transceiver type. A manager can legitimately be inspected before its
   * transceiver is wired up, and reporting that is `verifyAddresses`'s job, so
   * a missing one is not a construction error.
   */
  readonly transceiverAddress: string | undefined;
  readonly coreAddress: string;

  constructor(
    readonly network: N,
    readonly chain: C,
    readonly provider: SorobanRpc.Server,
    readonly contracts: Contracts & { ntt?: Ntt.Contracts }
  ) {
    if (!contracts.ntt) throw new Error(`NTT contracts for ${chain} not found`);
    if (!contracts.coreBridge)
      throw new Error(`CoreBridge address for ${chain} not found`);

    this.managerAddress = contracts.ntt.manager;
    this.tokenAddress = contracts.ntt.token;
    this.transceiverAddress = contracts.ntt.transceiver["wormhole"];
    this.coreAddress = contracts.coreBridge;
  }

  static async fromRpc<N extends Network>(
    provider: SorobanRpc.Server,
    config: ChainsConfig<N, StellarPlatformType>
  ): Promise<StellarNtt<N, StellarChains>> {
    const [network, chain] = await StellarPlatform.chainFromRpc(provider);
    const conf = config[chain]!;
    if (conf.network !== network)
      throw new Error(`Network mismatch: ${conf.network} !== ${network}`);
    return new StellarNtt(network as N, chain, provider, conf.contracts);
  }

  async getMode(): Promise<Ntt.Mode> {
    return decodeMode(await this.read("get_mode"));
  }

  async isPaused(): Promise<boolean> {
    return asBoolean(await this.read("paused"));
  }

  // The owner can be renounced, which leaves the manager permanently
  // unadministrable; the Ntt interface has no null case for it, so surface it.
  async getOwner(): Promise<AccountAddress<C>> {
    const owner = await this.read("get_owner");
    if (owner === null)
      throw new Error(`Ownership of ${this.managerAddress} was renounced`);
    return new StellarAddress(asAddress(owner)) as AccountAddress<C>;
  }

  async getPauser(): Promise<AccountAddress<C> | null> {
    const pauser = await this.read("get_pauser");
    return pauser === null
      ? null
      : (new StellarAddress(asAddress(pauser)) as AccountAddress<C>);
  }

  async getThreshold(): Promise<number> {
    return asNumber(await this.read("get_threshold"));
  }

  async getTokenDecimals(): Promise<number> {
    return asNumber(await this.read("token_decimals"));
  }

  /** The manager holds the locked tokens itself; in Burning mode it mints. */
  async getCustodyAddress(): Promise<string> {
    return this.managerAddress;
  }

  /** The Wormhole chain id the manager was deployed with; 61 for Stellar. */
  async getChainId(): Promise<number> {
    return asNumber(await this.read("get_chain_id"));
  }

  async getPeer<P extends Chain>(chain: P): Promise<Ntt.Peer<P> | null> {
    const peer = await this.read("get_peer", chainIdArg(chain));
    if (peer === null) return null;

    const { address, tokenDecimals, inboundRateLimit } =
      decodeNttManagerPeer(peer);
    return {
      // A peer address is the 32 wire bytes; on a Stellar peer that is a
      // one-way hash_address, so it stays universal rather than native.
      address: {
        chain,
        address: new UniversalAddress(new Uint8Array(address)),
      },
      tokenDecimals,
      inboundLimit: await this.untrim(inboundRateLimit.limit),
    };
  }

  /** Outbound capacity left right now, after the time-based refill. */
  async getCurrentOutboundCapacity(): Promise<bigint> {
    return this.untrim(asBigint(await this.read("get_outbound_capacity")));
  }

  async getOutboundLimit(): Promise<bigint> {
    return this.untrim(
      decodeRateLimitParams(await this.read("get_outbound_limit_params")).limit
    );
  }

  // Inbound rate limits live on the peer entry, so both of these report 0 for a
  // chain that is not a peer at all — it has no capacity rather than an empty
  // bucket. Use `getPeer` to tell the two apart.
  async getCurrentInboundCapacity(fromChain: Chain): Promise<bigint> {
    const capacity = await this.read(
      "get_inbound_capacity",
      chainIdArg(fromChain)
    );
    return capacity === null ? 0n : this.untrim(asBigint(capacity));
  }

  async getInboundLimit(fromChain: Chain): Promise<bigint> {
    const params = await this.read(
      "get_inbound_limit_params",
      chainIdArg(fromChain)
    );
    return params === null
      ? 0n
      : this.untrim(decodeRateLimitParams(params).limit);
  }

  /** Seconds, not milliseconds: Soroban ledger time. */
  async getRateLimitDuration(): Promise<bigint> {
    return asBigint(await this.read("get_rate_limit_duration"));
  }

  async getInboundQueuedTransfer(
    fromChain: Chain,
    transceiverMessage: Ntt.Message
  ): Promise<Ntt.InboundQueuedTransfer<C> | null> {
    const item = await this.read(
      "get_inbound_queue_item",
      bytesArg(Ntt.messageDigest(fromChain, transceiverMessage))
    );
    if (item === null) return null;

    const { recipient, amount, releaseTimestamp } =
      decodeInboundQueuedTransfer(item);
    return {
      recipient: new StellarAddress(recipient) as AccountAddress<C>,
      amount,
      // The contract stores the release time itself, so unlike EVM there is
      // nothing to add the rate-limit duration to.
      rateLimitExpiryTimestamp: Number(releaseTimestamp),
    };
  }

  /**
   * The outbound transfer queued under `sequence`, if the rate limiter held it
   * back. Stellar-specific: `complete_queued_transfer` and
   * `cancel_queued_transfer` are keyed by sequence, not by digest.
   */
  async getOutboundQueuedTransfer(
    sequence: bigint
  ): Promise<OutboundQueuedTransfer | null> {
    const item = await this.read(
      "get_outbound_queue_item",
      nativeToScVal(sequence, { type: "u64" })
    );
    return item === null ? null : decodeOutboundQueuedTransfer(item);
  }

  /** The threshold of attestations is met; the transfer may still be queued. */
  async getIsApproved(attestation: Ntt.Attestation): Promise<boolean> {
    return asBoolean(
      await this.read("is_message_approved", bytesArg(digestOf(attestation)))
    );
  }

  async getIsExecuted(attestation: Ntt.Attestation): Promise<boolean> {
    const executed = asBoolean(
      await this.read("is_message_executed", bytesArg(digestOf(attestation)))
    );
    // A rate-limited transfer is marked executed once the queue entry is
    // created, so it is only complete when nothing is left queued.
    return executed && !(await this.getIsTransferInboundQueued(attestation));
  }

  async getIsTransferInboundQueued(
    attestation: Ntt.Attestation
  ): Promise<boolean> {
    const [fromChain, message] = managerMessage(attestation);
    return (await this.getInboundQueuedTransfer(fromChain, message)) !== null;
  }

  /**
   * The cost of dispatching a transfer, in stroops: the sum of every enabled
   * transceiver's quote. A transceiver whose own quote failed reports `None`
   * and is skipped, so a broken transceiver does not lose the whole quote.
   */
  async quoteDeliveryPrice(
    destination: Chain,
    options: Ntt.TransferOptions
  ): Promise<bigint> {
    if (options.automatic)
      throw new Error("Relaying is not available on Stellar");

    const quotes = await this.read(
      "quote_delivery_price",
      chainIdArg(destination)
    );
    if (!Array.isArray(quotes))
      throw new Error(`Expected a Vec<TransceiverFee>, got ${typeof quotes}`);

    return quotes
      .map((quote) => decodeTransceiverFee(quote).fee ?? 0n)
      .reduce((total, fee) => total + fee, 0n);
  }

  /** Stellar has no NTT quoter, so a transfer is always manually redeemed. */
  async isRelayingAvailable(_destination: Chain): Promise<boolean> {
    return false;
  }

  /**
   * Locks or burns `amount` and dispatches it to `destination`.
   *
   * `options.queue` picks what happens when the outbound rate limiter is
   * exhausted: queue the transfer for later completion, or fail the whole call
   * with `TransferExceedsRateLimit`. `options.wrapNative` has nothing to do on
   * Stellar — XLM is already a SEP-41 contract, so the native token needs no
   * wrapper. An `additionalPayload` routes the call through
   * `transfer_with_payload` so the destination manager forwards it on.
   *
   * The manager takes custody with the token's own `transfer`/`burn`, both of
   * which authorize `sender` rather than an allowance, so no separate approve
   * step is needed. The transceiver pays the Wormhole message fee out of its
   * own balance — see {@link quoteDeliveryPrice} for what that costs.
   */
  async *transfer(
    sender: AccountAddress<C>,
    amount: bigint,
    destination: ChainAddress,
    options: Ntt.TransferOptions,
    additionalPayload?: Uint8Array
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    if (options.automatic)
      throw new Error("Relaying is not available on Stellar");

    const from = this.source(sender, "the sender");
    const args = [
      addressArg(from),
      nativeToScVal(amount, { type: "i128" }),
      chainIdArg(destination.chain),
      // Generic: the destination's own address type decides its wire form —
      // raw bytes for most chains, the one-way hash_address when the
      // destination is itself Stellar. The manager hashes `sender` and
      // `source_token` on-chain (outbound.rs), so neither is pre-hashed here.
      bytesArg(destination.address.toUniversalAddress().toUint8Array()),
      nativeToScVal(options.queue, { type: "bool" }),
    ];
    if (additionalPayload) args.push(bytesArg(additionalPayload));

    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call(
        additionalPayload ? "transfer_with_payload" : "transfer",
        ...args
      ),
      "Ntt.transfer"
    );
  }

  async verifyAddresses(): Promise<Partial<Ntt.Contracts> | null> {
    const [token, transceiver] = await Promise.all([
      this.read("get_token"),
      this.read("get_transceiver_info", nativeToScVal(0, { type: "u32" })),
    ]);

    const onChainToken = asAddress(token);
    const onChainTransceiver =
      transceiver === null
        ? undefined
        : decodeTransceiverInfo(transceiver).address;

    const mismatches: Partial<Ntt.Contracts> = {
      ...(onChainToken !== this.tokenAddress && { token: onChainToken }),
      ...(onChainTransceiver !== this.transceiverAddress && {
        transceiver: {
          ...(onChainTransceiver && { wormhole: onChainTransceiver }),
        },
      }),
    };
    return Object.keys(mismatches).length > 0 ? mismatches : null;
  }

  /**
   * Rescales a rate-limit amount into the token's own decimals.
   *
   * The manager consumes and stores rate limits in the trimmed domain
   * (`min(8, tokenDecimals)`, matching EVM's `_setOutboundLimit`), but the Ntt
   * interface reports amounts in token decimals. Unlike EVM's `TrimmedAmount`,
   * Soroban's `RateLimitParams` is a bare `u64` that carries no decimals, so
   * there is nothing to read back and the scale has to be reconstructed.
   */
  private async untrim(trimmed: bigint): Promise<bigint> {
    const decimals = await this.getTokenDecimals();
    return trimmed * 10n ** BigInt(Math.max(0, decimals - TRIMMED_DECIMALS));
  }

  /** Simulate a read-only manager call and return its decoded result. */
  private read(method: string, ...args: xdr.ScVal[]): Promise<unknown> {
    return StellarPlatform.simulateRead(
      this.provider,
      this.network,
      this.managerAddress,
      method,
      ...args
    );
  }

  /**
   * The account a write is sent from, which is also the account it authorizes.
   *
   * Every NTT write requires exactly one address to have authorized it — the
   * sender, the owner, the pauser. Soroban's simulation returns source-account
   * credentials for an address that is also the transaction source, and those
   * are covered by the envelope signature the signer already applies; any other
   * address would need its auth entry signed separately. So the caller's payer
   * has to *be* the authorizing address, and there is no default for it.
   */
  private source(payer: AccountAddress<C> | undefined, role: string): string {
    if (payer === undefined)
      throw new Error(`Stellar has no implicit signer: pass ${role} as payer`);
    return new StellarAddress(payer as AnyStellarAddress).toString();
  }

  /** Build, simulate and assemble a write for the signer to sign and submit. */
  private async prepare(
    from: string,
    operation: xdr.Operation,
    description: string,
    space: ContractErrorSpace = "NttManager"
  ): Promise<StellarUnsignedTransaction<N, C>> {
    const source = await this.provider.getAccount(from);
    const tx = new TransactionBuilder(source, {
      fee: BASE_FEE,
      networkPassphrase: stellarNetworkPassphrase(this.network),
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const prepared = await this.provider
      .prepareTransaction(tx)
      .catch((error: unknown) => {
        throw decodeContractError(error, space);
      });

    return new StellarUnsignedTransaction(
      prepared,
      this.network,
      this.chain,
      description,
      // Connect runs non-parallelizable transactions one at a time, waiting for
      // each to confirm — which is what keeps a queued transfer's completion
      // behind the transfer that queued it.
      false
    );
  }
}

/** `TrimmedAmount::MAX_DECIMALS` — the ceiling of the shared NTT amount domain. */
const TRIMMED_DECIMALS = 8;

/** Chain ids cross the Soroban ABI as `u32`, though the wire format is `u16`. */
const chainIdArg = (chain: Chain): xdr.ScVal =>
  nativeToScVal(toChainId(chain), { type: "u32" });

/** `Bytes` and `BytesN<N>` share one ScVal type; the host checks the length. */
const bytesArg = (bytes: Uint8Array): xdr.ScVal =>
  nativeToScVal(Buffer.from(bytes), { type: "bytes" });

const addressArg = (address: AnyStellarAddress): xdr.ScVal =>
  new Address(new StellarAddress(address).toString()).toScVal();

/**
 * The manager message an attestation carries, with the chain that sent it.
 * A standard-relayer VAA wraps the transceiver message one level deeper.
 */
const managerMessage = (attestation: Ntt.Attestation): [Chain, Ntt.Message] => [
  attestation.emitterChain,
  (attestation.payloadName === "WormholeTransfer"
    ? attestation.payload
    : attestation.payload["payload"])["nttManagerPayload"],
];

const digestOf = (attestation: Ntt.Attestation): Uint8Array =>
  Ntt.messageDigest(...managerMessage(attestation));
