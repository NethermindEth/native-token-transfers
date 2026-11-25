import {
  Address,
  BASE_FEE,
  Contract,
  StrKey,
  TransactionBuilder,
  nativeToScVal,
  rpc as SorobanRpc,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import type { Operation } from "@stellar/stellar-sdk";
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
  nativeSacId,
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
  addressArg,
  asBigint,
  asBoolean,
  asNumber,
  asAddress,
  bytesArg,
  chainIdArg,
  decodeInboundQueuedTransfer,
  decodeMode,
  decodeNttManagerPeer,
  decodeOutboundQueuedTransfer,
  decodeRateLimitParams,
  decodeTransceiverFee,
  decodeTransceiverInfo,
  sequenceArg,
} from "./scval-types.js";
import type { OutboundQueuedTransfer } from "./scval-types.js";
import { StellarNttWormholeTransceiver } from "./transceiver.js";

/**
 * A call in a write's auth tree: the contract, the method, the arguments as
 * `scValToNative` decodes them, and the calls it authorizes in turn.
 */
export interface AuthorizedCall {
  contract: string;
  method: string;
  args: unknown[];
  children: AuthorizedCall[];
}

/**
 * Checks the calls a write's auth tree authorizes below the call that was
 * built, and throws on any the write has no reason to authorize.
 */
export type AuthPolicy = (children: AuthorizedCall[]) => Promise<void>;

/**
 * NTT bindings for the Soroban manager + Wormhole transceiver contracts.
 *
 * Every read is a read-only `simulateTransaction` against the manager
 * (`StellarPlatform.simulateRead`), and every write yields a prepared,
 * non-parallelizable `StellarUnsignedTransaction` for the signer to submit.
 */
export class StellarNtt<N extends Network, C extends StellarChains>
  implements Ntt<N, C>
{
  readonly managerAddress: string;
  readonly tokenAddress: string;
  /**
   * The Wormhole transceiver, if the config names one; Stellar registers no
   * other transceiver type. A caller can legitimately inspect a manager before
   * its transceiver is registered, and reporting that is the `verifyAddresses`
   * method's job, so a missing one is not a construction error.
   */
  readonly transceiverAddress: string | undefined;
  readonly coreAddress: string;
  /**
   * The highest fee, in stroops, that {@link prepare} accepts. The RPC sets the
   * resource fee from its simulation, with no cap of its own, so raise this
   * only for a write that is known to cost more. It also caps, separately, the
   * XLM a transfer pays its transceivers (see {@link transferPolicy}). Default:
   * 50_000_000 (5 XLM).
   */
  maxFee = 50_000_000n;

  constructor(
    readonly network: N,
    readonly chain: C,
    readonly provider: SorobanRpc.Server,
    readonly contracts: Contracts & { ntt?: Ntt.Contracts }
  ) {
    if (!contracts.ntt) throw new Error(`NTT contracts for ${chain} not found`);
    if (!contracts.coreBridge)
      throw new Error(`CoreBridge address for ${chain} not found`);
    for (const [field, address] of [
      ["ntt.manager", contracts.ntt.manager],
      ["ntt.token", contracts.ntt.token],
      ["coreBridge", contracts.coreBridge],
      ["ntt.transceiver.wormhole", contracts.ntt.transceiver["wormhole"]],
    ] as const)
      if (address !== undefined && !StrKey.isValidContract(address))
        throw new Error(`${field} for ${chain} is not a contract id`);

    this.managerAddress = contracts.ntt.manager;
    this.tokenAddress = contracts.ntt.token;
    this.transceiverAddress = contracts.ntt.transceiver["wormhole"];
    this.coreAddress = contracts.coreBridge;
  }

  /**
   * Creates a binding for the chain the RPC endpoint serves, taking contract
   * addresses from `config`. Throws if that config names a different network
   * than the endpoint reports.
   */
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

  // The owner can renounce ownership, which leaves the manager permanently
  // unadministrable; the `Ntt` interface has no null case for it, so surface it.
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

  /**
   * Returns the manager's own address: it holds the locked tokens, and in
   * Burning mode it mints.
   */
  async getCustodyAddress(): Promise<string> {
    return this.managerAddress;
  }

  /** Returns the Wormhole chain id the manager was deployed with; 61 for Stellar. */
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
      // one-way `hash_address`, so it stays universal rather than native.
      address: {
        chain,
        address: new UniversalAddress(new Uint8Array(address)),
      },
      tokenDecimals,
      inboundLimit: await this.untrim(inboundRateLimit.limit),
    };
  }

  /**
   * Returns the outbound capacity left at the current ledger, after the
   * time-based refill.
   */
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

  /** Returns the duration in seconds, not milliseconds: Soroban ledger time. */
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
   * Returns the outbound transfer queued under `sequence`, if the rate limiter
   * held it back. Stellar-specific: `complete_queued_transfer` and
   * `cancel_queued_transfer` are keyed by sequence, not by digest.
   */
  async getOutboundQueuedTransfer(
    sequence: bigint
  ): Promise<OutboundQueuedTransfer | null> {
    const item = await this.read(
      "get_outbound_queue_item",
      sequenceArg(sequence)
    );
    return item === null ? null : decodeOutboundQueuedTransfer(item);
  }

  /**
   * Releases an inbound transfer the rate limiter held back. Permissionless
   * once the queue entry's `release_timestamp` has passed — `payer` only funds
   * the transaction — and the tokens go to the recipient the message named,
   * not to whoever calls this.
   */
  async *completeInboundQueuedTransfer(
    fromChain: Chain,
    transceiverMessage: Ntt.Message,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.prepare(
      this.source(payer, "the payer"),
      new Contract(this.managerAddress).call(
        "complete_inbound_transfer",
        bytesArg(Ntt.messageDigest(fromChain, transceiverMessage))
      ),
      "Ntt.completeInboundQueuedTransfer"
    );
  }

  /**
   * Dispatches an outbound transfer the rate limiter held back, keyed by the
   * sequence {@link getOutboundQueuedTransfer} reports. Also permissionless
   * once the delay has been served; the message is sent under the same sequence.
   * `payer` authorizes the call and pays every transceiver's quote in XLM.
   */
  async *completeOutboundQueuedTransfer(
    sequence: bigint,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.source(payer, "the payer");
    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call(
        "complete_queued_transfer",
        addressArg(from),
        sequenceArg(sequence)
      ),
      "Ntt.completeOutboundQueuedTransfer",
      "NttManager",
      this.transferPolicy(from, 0n)
    );
  }

  /**
   * Abandons a queued outbound transfer and refunds the custodied tokens.
   * Only the original sender may cancel (`CancellerNotSender` otherwise), so
   * `sender` is both the authorizing address and the refund recipient.
   */
  async *cancelOutboundQueuedTransfer(
    sequence: bigint,
    sender: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.source(sender, "the original sender");
    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call(
        "cancel_queued_transfer",
        addressArg(from),
        sequenceArg(sequence)
      ),
      "Ntt.cancelOutboundQueuedTransfer"
    );
  }

  /**
   * Checks whether the threshold of attestations is met; the transfer may
   * still be queued.
   */
  async getIsApproved(attestation: Ntt.Attestation): Promise<boolean> {
    return asBoolean(
      await this.read("is_message_approved", bytesArg(digestOf(attestation)))
    );
  }

  async getIsExecuted(attestation: Ntt.Attestation): Promise<boolean> {
    const executed = asBoolean(
      await this.read("is_message_executed", bytesArg(digestOf(attestation)))
    );
    // The manager marks a rate-limited transfer executed once it creates the
    // queue entry, so the transfer is only complete when nothing is left queued.
    return executed && !(await this.getIsTransferInboundQueued(attestation));
  }

  async getIsTransferInboundQueued(
    attestation: Ntt.Attestation
  ): Promise<boolean> {
    const [fromChain, message] = managerMessage(attestation);
    return (await this.getInboundQueuedTransfer(fromChain, message)) !== null;
  }

  /**
   * Returns the cost of dispatching a transfer, in stroops: the sum of every
   * enabled transceiver's quote. Throws if a transceiver could not quote,
   * because the manager pays every quote and the transfer would fail too.
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
      .map((quote) => {
        const { transceiver, fee } = decodeTransceiverFee(quote);
        if (fee === null)
          throw new Error(`Transceiver ${transceiver} could not quote a fee`);
        return fee;
      })
      .reduce((total, fee) => total + fee, 0n);
  }

  /**
   * Returns `false`: Stellar has no NTT quoter, so every transfer needs a
   * manual redeem.
   */
  async isRelayingAvailable(_destination: Chain): Promise<boolean> {
    return false;
  }

  /**
   * Locks or burns `amount` and dispatches it to `destination`.
   *
   * `options.queue` controls what happens when the outbound rate limiter is
   * exhausted: queue the transfer for later completion, or fail the whole call
   * with `TransferExceedsRateLimit`. `options.wrapNative` has nothing to do on
   * Stellar — XLM is already a SEP-41 contract, so the native token needs no
   * wrapper. An `additionalPayload` routes the call through
   * `transfer_with_payload` so the destination manager forwards it on.
   *
   * The manager takes custody with the token's own `transfer`/`burn`, both of
   * which authorize `sender` rather than an allowance, so the caller needs no
   * separate approve step. The sender also pays every transceiver's quote in
   * XLM (see {@link quoteDeliveryPrice}); a queued transfer charges whoever
   * completes it instead.
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
      // Generic: the destination's own address type determines its wire form —
      // raw bytes for most chains, the one-way `hash_address` when the
      // destination is itself Stellar. The manager hashes `sender` and
      // `source_token` on-chain (`outbound.rs`), so this call passes both
      // unhashed.
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
      "Ntt.transfer",
      "NttManager",
      this.transferPolicy(from, amount)
    );
  }

  // Administration. Each of these authorizes exactly one address — the owner,
  // except where noted — so `payer` has to be that address, not a third party
  // funding it. See `source` for why.

  /**
   * Registers or updates the manager on `peer.chain`. `tokenDecimals` are the
   * peer token's, used to normalize amounts; `inboundLimit` is in this token's
   * decimals, and `trimArg` trims it on the way in (see {@link setInboundLimit}).
   */
  async *setPeer(
    peer: ChainAddress,
    tokenDecimals: number,
    inboundLimit: bigint,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.setPeer",
      "set_peer",
      chainIdArg(peer.chain),
      bytesArg(peer.address.toUniversalAddress().toUint8Array()),
      nativeToScVal(tokenDecimals, { type: "u32" }),
      await this.trimArg(inboundLimit)
    );
  }

  /**
   * Forwards to {@link StellarNttWormholeTransceiver.setPeer} under the `Ntt`
   * interface's name for it.
   */
  async *setTransceiverPeer(
    ix: number,
    peer: ChainAddress,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield* this.transceiver(ix).setPeer(peer, payer);
  }

  /** Sets how many transceivers must attest before an inbound transfer executes. */
  async *setThreshold(
    threshold: number,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.setThreshold",
      "set_threshold",
      nativeToScVal(threshold, { type: "u32" })
    );
  }

  /**
   * Registers a transceiver and assigns it its permanent bitmap index.
   * Registering the first one raises the threshold from 0 to 1 by itself.
   */
  async *setTransceiver(
    transceiver: AnyStellarAddress,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.setTransceiver",
      "set_transceiver",
      addressArg(transceiver)
    );
  }

  /**
   * Disables a transceiver, lowering the threshold if it no longer fits. The
   * registry entry and its index survive, so the owner can re-enable it; the
   * contract rejects removing the last enabled transceiver.
   */
  async *removeTransceiver(
    transceiver: AnyStellarAddress,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.removeTransceiver",
      "remove_transceiver",
      addressArg(transceiver)
    );
  }

  /** Sets the outbound rate limit, in the token's decimals (see {@link trimArg}). */
  async *setOutboundLimit(
    limit: bigint,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.setOutboundLimit",
      "set_outbound_limit",
      await this.trimArg(limit)
    );
  }

  /**
   * Sets the inbound rate limit for `fromChain`, in the token's decimals (see
   * {@link trimArg}).
   */
  async *setInboundLimit(
    fromChain: Chain,
    limit: bigint,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(
      payer,
      "Ntt.setInboundLimit",
      "set_inbound_limit",
      chainIdArg(fromChain),
      await this.trimArg(limit)
    );
  }

  /**
   * Proposes `newOwner`, who then has to call {@link acceptOwnership} before
   * `liveUntilLedger` — a two-step handover, so a typo cannot strand the
   * manager. Defaults to roughly a day of ledgers; passing `0` cancels a
   * pending proposal instead.
   */
  async *setOwner(
    newOwner: AccountAddress<C>,
    payer?: AccountAddress<C>,
    liveUntilLedger?: number
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const until =
      liveUntilLedger ??
      (await this.provider.getLatestLedger()).sequence +
        OWNERSHIP_OFFER_LEDGERS;
    yield await this.admin(
      payer,
      "Ntt.setOwner",
      "transfer_ownership",
      addressArg(newOwner),
      nativeToScVal(until, { type: "u32" })
    );
  }

  /**
   * Completes the two-step handover {@link setOwner} proposed, authorized by the
   * proposed owner.
   */
  async *acceptOwnership(
    newOwner: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.admin(newOwner, "Ntt.acceptOwnership", "accept_ownership");
  }

  /**
   * Hands the pause capability to `newPauser`, or removes the role entirely
   * with `null`, leaving `pause` owner-only. Authorized by the owner *or* the
   * current pauser.
   */
  async *setPauser(
    newPauser: AccountAddress<C> | null,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.source(payer, "the owner or the current pauser");
    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call(
        "transfer_pauser",
        addressArg(from),
        // `Option<Address>`: `None` crosses the ABI as the unit value.
        newPauser === null ? xdr.ScVal.scvVoid() : addressArg(newPauser)
      ),
      "Ntt.setPauser"
    );
  }

  /**
   * Pauses the manager as an emergency stop, available to the owner and the
   * pauser alike.
   */
  async *pause(
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.source(payer, "the owner or the pauser");
    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call("pause", addressArg(from)),
      "Ntt.pause"
    );
  }

  /**
   * Resumes the manager. Owner-only, unlike {@link pause}: a compromised
   * pauser cannot resume.
   */
  async *unpause(
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.source(payer, "the owner");
    yield await this.prepare(
      from,
      new Contract(this.managerAddress).call("unpause", addressArg(from)),
      "Ntt.unpause"
    );
  }

  /**
   * Delivers each attestation to the Wormhole transceiver, which verifies the
   * VAA against the core and forwards it to the manager. One transaction per
   * attestation: the transceiver takes a single VAA, and each carries its own
   * replay protection, so the caller can resume a batch that fails part-way.
   */
  async *redeem(
    attestations: Ntt.Attestation[],
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const transceiver = this.transceiver(0);
    for (const attestation of attestations)
      yield* transceiver.receive(attestation, payer);
  }

  /**
   * Registers `address` on the Wormhole core's address registry, so an inbound
   * transfer can resolve its hashed recipient back to a typed `G…`/`C…`.
   *
   * Permissionless: `payer` only funds the transaction and need not be the
   * address being recorded. Until this transaction confirms, a transfer to
   * that recipient fails with `RecipientNotRegistered`; the entry is persistent
   * storage, so a long-idle registration can also need restoring.
   */
  async *recordAddress(
    payer: AccountAddress<C>,
    address: AnyStellarAddress
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.prepare(
      this.source(payer, "the payer"),
      new Contract(this.coreAddress).call(
        "record_address",
        addressArg(address)
      ),
      "Ntt.recordAddress",
      "WormholeCore"
    );
  }

  /**
   * Gets the transceiver at `ix`, or `null` if the manager has none there.
   * Stellar registers only the Wormhole transceiver, at index 0. Unlike the
   * private accessor the write paths use, this reports a missing one rather
   * than throwing: asking which transceivers exist is a legitimate question.
   */
  async getTransceiver(
    ix: number
  ): Promise<StellarNttWormholeTransceiver<N, C> | null> {
    if (ix !== 0 || this.transceiverAddress === undefined) return null;
    return this.transceiver(ix);
  }

  /**
   * Compares the configured token and transceiver against the manager's own,
   * returning the fields that differ, or `null` when every one of them
   * matches.
   */
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
   * (`min(8, tokenDecimals)`, matching EVM's `_setOutboundLimit`), but the
   * `Ntt` interface reports amounts in token decimals. Unlike EVM's
   * `TrimmedAmount`, Soroban's `RateLimitParams` is a bare `u64` that carries no
   * decimals, so there is nothing to read back and this binding has to
   * reconstruct the scale.
   */
  private async untrim(trimmed: bigint): Promise<bigint> {
    const decimals = await this.getTokenDecimals();
    return trimmed * 10n ** BigInt(Math.max(0, decimals - TRIMMED_DECIMALS));
  }

  /**
   * Returns the transceiver at `ix`; Stellar registers only the Wormhole
   * transceiver, and only at index 0.
   */
  private transceiver(ix: number): StellarNttWormholeTransceiver<N, C> {
    if (ix !== 0 || this.transceiverAddress === undefined)
      throw new Error(`No transceiver at index ${ix} for ${this.chain}`);
    return new StellarNttWormholeTransceiver(this, this.transceiverAddress);
  }

  /** Prepares an owner-authorized manager call, sent from the owner's own account. */
  private async admin(
    payer: AccountAddress<C> | undefined,
    description: string,
    method: string,
    ...args: xdr.ScVal[]
  ): Promise<StellarUnsignedTransaction<N, C>> {
    return this.prepare(
      this.source(payer, "the owner"),
      new Contract(this.managerAddress).call(method, ...args),
      description
    );
  }

  /**
   * Rescales a rate limit from the token's decimals into the trimmed domain
   * the manager stores it in — the exact inverse of {@link untrim}, which
   * explains why the scale has to be reconstructed at all.
   * Skipping this would set every limit `10^(decimals - 8)` too high.
   *
   * Rounds down: a limit that is not a whole number of trimmed units cannot be
   * represented, and rounding up would raise the limit that was asked for.
   */
  private async trimArg(amount: bigint): Promise<xdr.ScVal> {
    const decimals = await this.getTokenDecimals();
    const trimmed =
      amount / 10n ** BigInt(Math.max(0, decimals - TRIMMED_DECIMALS));
    if (trimmed > MAX_U64)
      throw new Error(`Rate limit ${amount} overflows the manager's u64`);
    // A limit under one trimmed unit would floor to zero, which the rate
    // limiter reads as "allow nothing" — not what a small limit asked for.
    if (amount > 0n && trimmed === 0n)
      throw new Error(
        `Rate limit ${amount} is below one trimmed unit of a ${decimals}-decimal token`
      );
    return nativeToScVal(trimmed, { type: "u64" });
  }

  /**
   * Returns the policy of a manager call that moves `amount` of the token out
   * of `from` and pays the transceivers: `transfer`, `transfer_with_payload`,
   * or `complete_queued_transfer` with an `amount` of 0.
   *
   * Below that call, `from` authorizes only the token's `transfer` or `burn`
   * and native `transfer`s, each out of `from` and with nothing below it. The
   * token goes only to the manager, as custody, and XLM only to the configured
   * transceiver, as its fee, so a manager that pays any other transceiver is
   * rejected; when the token is XLM, its custody is the native `transfer` to
   * the manager. They move at most `amount` of the token, and at most
   * {@link maxFee} in XLM. That bound is the caller's own rather than a read:
   * the core's message fee and the manager's transceiver count would come from
   * the RPC and from the contracts this check distrusts.
   *
   * Public, like {@link prepare}, because the executor binding checks the
   * moves of the transfer it wraps with this policy.
   */
  transferPolicy(from: string, amount: bigint): AuthPolicy {
    return async (moves) => {
      const native = nativeSacId(this.network);
      let token = 0n;
      let stroops = 0n;
      for (const move of moves) {
        const { contract, method, args, children } = move;
        const allowed =
          contract === native
            ? method === "transfer"
            : contract === this.tokenAddress &&
              (method === "transfer" || method === "burn");
        if (!allowed || children.length > 0) throw unexpectedCall(move);

        const sender = asAddress(args[0]);
        const value = asBigint(args[method === "burn" ? 1 : 2]);
        if (value < 0n)
          throw new Error(`The auth tree moves ${value} of ${contract}`);
        if (sender !== from)
          throw new Error(
            `The auth tree moves ${contract} from ${sender}; ` +
              `a transfer moves only from ${from}`
          );
        // A `burn` has no recipient: the manager takes custody by burning.
        const to = method === "burn" ? this.managerAddress : asAddress(args[1]);
        if (contract === this.tokenAddress && to === this.managerAddress)
          token += value;
        else if (contract === native && to === this.transceiverAddress)
          stroops += value;
        else
          throw new Error(
            `The auth tree moves ${contract} to ${to}; a transfer moves the ` +
              `token only to the manager ${this.managerAddress}, and XLM only ` +
              `to the transceiver ${this.transceiverAddress ?? "(none configured)"}`
          );
      }

      if (token > amount)
        throw new Error(
          `The auth tree moves ${token} of the token, above the ${amount} sent`
        );
      if (stroops > this.maxFee)
        throw new Error(
          `The auth tree moves ${stroops} stroops of XLM to the transceiver, ` +
            `above maxFee (${this.maxFee})`
        );
    };
  }

  /** Simulates a read-only manager call and returns its decoded result. */
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
   * Returns the account that sources a write, which is also the account it
   * authorizes.
   *
   * Every NTT write requires exactly one address to have authorized it — the
   * sender, the owner, the pauser. Soroban's simulation returns source-account
   * credentials for an address that is also the transaction source, and the
   * envelope signature the signer already applies covers those; any other
   * address would need a separately signed auth entry. So the caller's payer
   * has to *be* the authorizing address, and there is no default for it.
   *
   * That address also has to be a `G…` account, because only an account can
   * source a transaction. To administer a manager owned by a `C…` contract,
   * invoke that contract rather than going through this class.
   *
   * Public, like {@link prepare}, because the transceiver binding sends its
   * own contract's calls through this one — the manager owns the package's
   * only transaction builder, exactly as `EvmNtt.createUnsignedTx` does.
   */
  source(payer: AccountAddress<C> | undefined, role: string): string {
    if (payer === undefined)
      throw new Error(`Stellar has no implicit signer: pass ${role} as payer`);

    const from = new StellarAddress(payer as AnyStellarAddress).toString();
    if (!from.startsWith("G"))
      throw new Error(
        `${role} must be a G… account to source a transaction, got ${from}`
      );
    return from;
  }

  /**
   * Builds, simulates, and assembles a write for the signer to sign and submit.
   * `space` names the contract whose error table decodes a rejection, so a
   * rejection from another NTT contract does not carry a manager error name.
   *
   * The source account's envelope signature authorizes every source-account
   * entry the simulation recorded, so this throws unless there is at most one
   * entry, for the source account, rooted at the call that was built. `policy`
   * checks the calls below that root; by default there may be none. It also
   * throws if the fee is above {@link maxFee}.
   */
  async prepare(
    from: string,
    operation: xdr.Operation,
    description: string,
    space: ContractErrorSpace = "NttManager",
    policy: AuthPolicy = noChildren
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

    if (BigInt(prepared.fee) > this.maxFee) {
      // A write restores the archived entries it touches, and its resource
      // fee pays for that; the simulation lists them in the transaction data.
      const ext = prepared.toEnvelope().v1().tx().ext().sorobanData().ext();
      const restores =
        ext.switch() === 1 &&
        ext.resourceExt().archivedSorobanEntries().length > 0;
      throw new Error(
        `${description} costs ${prepared.fee} stroops, above maxFee (${this.maxFee})` +
          (restores
            ? ", because it restores archived ledger entries. Restore them " +
              "in a separate restoreFootprint transaction, then prepare it again"
            : "")
      );
    }

    const [invoke] = prepared.operations;
    if (invoke?.type === "invokeHostFunction")
      await checkAuth(invoke, from, description, policy);

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

/**
 * Checks the auth entries the simulation recorded for `invoke`.
 *
 * The signer signs only the envelope, which covers source-account
 * credentials; any other address would have to sign its own entry, and the
 * transaction would fail on chain without it. A write authorizes one call, so
 * there is at most one entry, and its root is the call `invoke` makes rather
 * than one the simulation chose. `policy` then checks every call below it.
 */
async function checkAuth(
  invoke: Operation.InvokeHostFunction,
  from: string,
  description: string,
  policy: AuthPolicy
): Promise<void> {
  const entries = invoke.auth ?? [];
  for (const entry of entries) {
    const credentials = entry.credentials();
    // By value: the provider's stellar-sdk need not be this package's copy,
    // and each copy has its own enum objects.
    if (
      credentials.switch().value ===
      xdr.SorobanCredentialsType.sorobanCredentialsAddress().value
    )
      throw new Error(
        `${description} needs an auth entry signed by ` +
          `${Address.fromScAddress(credentials.address().address()).toString()}, ` +
          `and only the source account ${from} signs this transaction`
      );
  }

  if (entries.length > 1)
    throw new Error(
      `${description} recorded ${entries.length} auth entries; a write authorizes one`
    );
  const [entry] = entries;
  if (entry === undefined) return;

  const root = entry.rootInvocation();
  const call =
    xdr.SorobanAuthorizedFunction.sorobanAuthorizedFunctionTypeContractFn(
      invoke.func.invokeContract()
    );
  if (root.function().toXDR("base64") !== call.toXDR("base64"))
    throw new Error(
      `${description} recorded an auth entry for a call other than the one it makes`
    );
  await policy(root.subInvocations().map(decodeCall));
}

/** Decodes an auth tree; anything but a contract call throws. */
const decodeCall = (
  invocation: xdr.SorobanAuthorizedInvocation
): AuthorizedCall => {
  const call = invocation.function().contractFn();
  return {
    contract: Address.fromScAddress(call.contractAddress()).toString(),
    method: call.functionName().toString(),
    args: call.args().map((arg) => scValToNative(arg)),
    children: invocation.subInvocations().map(decodeCall),
  };
};

/** The policy of a write that authorizes nothing below its own call. */
const noChildren: AuthPolicy = async ([call]) => {
  if (call !== undefined) throw unexpectedCall(call);
};

/** Names a call that a write has no reason to authorize. */
const unexpectedCall = ({ contract, method }: AuthorizedCall): Error =>
  new Error(`Unexpected call in the auth tree: ${contract}.${method}`);

/** `TrimmedAmount::MAX_DECIMALS` — the ceiling of the shared NTT amount domain. */
const TRIMMED_DECIMALS = 8;

/** The manager stores rate limits as a bare `u64`, so a trimmed limit has to fit one. */
const MAX_U64 = 2n ** 64n - 1n;

/** Roughly a day of ~5-second ledgers, to accept an ownership offer in. */
const OWNERSHIP_OFFER_LEDGERS = 17280;

/**
 * Returns the manager message an attestation carries, with the chain that sent it.
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
