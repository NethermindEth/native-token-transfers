import {
  Contract,
  StrKey,
  nativeToScVal,
  rpc as SorobanRpc,
} from "@stellar/stellar-sdk";
import type { Network } from "@wormhole-foundation/sdk-base";
import type {
  AccountAddress,
  ChainAddress,
  ChainsConfig,
  Contracts,
} from "@wormhole-foundation/sdk-definitions";
import type {
  Ntt,
  NttWithExecutor,
} from "@wormhole-foundation/sdk-definitions-ntt";
import {
  StellarAddress,
  StellarPlatform,
} from "@wormhole-foundation/sdk-stellar";
import type {
  StellarChains,
  StellarPlatformType,
  StellarUnsignedTransaction,
} from "@wormhole-foundation/sdk-stellar";
import { STELLAR_ADDRESSES } from "./constants.js";
import { StellarNtt } from "./ntt.js";
import { addressArg, bytesArg, chainIdArg, structArg } from "./scval-types.js";

/**
 * Bindings for the `ntt-with-executor` wrapper: one Soroban call that transfers
 * through an NTT manager and pays the Wormhole Executor to deliver the message.
 *
 * The wrapper exists because a Stellar transaction permits only one top-level
 * `InvokeHostFunction`, so a client cannot batch the manager transfer and
 * `request_execution` the way Sui's PTB or an EVM multicall can. Everything it
 * does runs under the single auth tree rooted at the sender, and it takes no
 * custody of its own.
 *
 * The wrapper binds the executor contract at construction, so a caller only
 * ever names the wrapper; the manager is a per-call argument, which is why one
 * wrapper serves every manager on the chain.
 */
export class StellarNttWithExecutor<N extends Network, C extends StellarChains>
  implements NttWithExecutor<N, C>
{
  /**
   * `wrapperAddress` overrides {@link STELLAR_ADDRESSES}, which is how a
   * localnet or a private deployment names its own wrapper — the table carries
   * an entry only where a public deployment exists, and an invented address is
   * worse than none.
   */
  constructor(
    readonly network: N,
    readonly chain: C,
    readonly provider: SorobanRpc.Server,
    readonly contracts: Contracts & { ntt?: Ntt.Contracts },
    readonly wrapperAddress?: string
  ) {}

  /**
   * Creates a binding for the chain the RPC endpoint serves.
   *
   * The wrapper address comes from {@link STELLAR_ADDRESSES}, because the chain
   * configuration carries no field for it. Throws if that config names a
   * different network than the endpoint reports.
   */
  static async fromRpc<N extends Network>(
    provider: SorobanRpc.Server,
    config: ChainsConfig<N, StellarPlatformType>
  ): Promise<StellarNttWithExecutor<N, StellarChains>> {
    const [network, chain] = await StellarPlatform.chainFromRpc(provider);
    const conf = config[chain]!;
    if (conf.network !== network)
      throw new Error(`Network mismatch: ${conf.network} !== ${network}`);
    return new StellarNttWithExecutor(
      network as N,
      chain,
      provider,
      conf.contracts
    );
  }

  /**
   * Transfers `amount` through `ntt` and pays the executor to relay it.
   *
   * `amount` is the **whole** amount, referrer fee included: the wrapper takes
   * the fee out of it on-chain from `quote.referrerFeeDbps` and bridges the
   * rest, so `quote.referrerFee` and `quote.remainingAmount` are the route's
   * preview of that same split rather than inputs. `remainingAmount` is exact —
   * both sides truncate the fee to `min(8, sourceDecimals, peerDecimals)`
   * decimals — while `referrerFee` is the route's *untrimmed* figure and can
   * read slightly high.
   *
   * `sender` alone authorizes every token movement — the referrer fee, the
   * manager's lock or burn, and the executor's XLM fee — and also sources the
   * transaction, so the envelope signature covers the whole auth tree and no
   * auth entry needs a separate signature.
   *
   * `wrapNative` has nothing to do here: XLM is already a SEP-41 contract on
   * Stellar, so the native token needs no wrapper. The contract also does not
   * expose `should_queue` — it forces `false`, because a rate-limited transfer
   * emits no message and paying a relayer to deliver nothing would be premature.
   */
  async *transfer(
    sender: AccountAddress<C>,
    destination: ChainAddress,
    amount: bigint,
    quote: NttWithExecutor.Quote,
    ntt: StellarNtt<N, C>,
    _wrapNative: boolean = false
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    // This check names a stale quote as such, rather than letting it arrive as
    // `ExecutorError::QuoteExpired (11)` out of a simulation.
    if (new Date() >= quote.expires)
      throw new Error(`Quote expired at ${quote.expires.toISOString()}`);

    // `dbps` crosses the ABI as a `u32` but has to fit the on-wire `u16` fee
    // field, which is also what keeps the `Number` conversion below in range.
    if (quote.referrerFeeDbps < 0n || quote.referrerFeeDbps > MAX_U16)
      throw new Error(
        `Referrer fee ${quote.referrerFeeDbps} dbps does not fit the u16 wire field`
      );

    // The wrapper pays the fee on Stellar in the transfer token, with a plain
    // `token.transfer`, so the referrer has to be an address this chain can
    // name. A Stellar `UniversalAddress` is the one-way `hash_address` and is no
    // help, so require the native form rather than trying to invert it.
    const { address: referrer } = quote.referrer;
    if (!StellarAddress.instanceof(referrer))
      throw new Error(
        `The referrer fee is paid on ${this.chain}, so quote.referrer must be a ` +
          `native Stellar address; got ${referrer.toString()} on ${quote.referrer.chain}`
      );

    // Everything that can be settled without the network is settled first, so
    // an unusable call fails on what is wrong with it rather than on an RPC.
    const wrapper = this.wrapper();
    const from = ntt.source(sender, "the sender");
    const payee = await this.payee(quote.payeeAddress);

    yield await ntt.prepare(
      from,
      new Contract(wrapper).call(
        "transfer",
        addressArg(from),
        addressArg(ntt.managerAddress),
        nativeToScVal(amount, { type: "i128" }),
        structArg({
          chain: chainIdArg(destination.chain),
          // Generic, exactly as `StellarNtt.transfer` does it: the destination's
          // own address type determines its wire form.
          recipient: bytesArg(
            destination.address.toUniversalAddress().toUint8Array()
          ),
        }),
        structArg({
          dbps: nativeToScVal(Number(quote.referrerFeeDbps), { type: "u32" }),
          referrer: addressArg(referrer),
        }),
        structArg({
          amount: nativeToScVal(quote.estimatedCost, { type: "i128" }),
          payee: addressArg(payee),
          // The executor moves an exact `amount`, so nothing is left to refund
          // on this side; the field is recorded in the request event for the
          // relay provider, and the sender is who a refund is owed to. EVM
          // passes its sender here for the same reason.
          refund: addressArg(from),
          relay_instructions: bytesArg(quote.relayInstructions),
          signed_quote: bytesArg(quote.signedQuote),
        })
      ),
      "NttWithExecutor.transfer",
      "NttWithExecutor"
    );
  }

  /**
   * Returns zero on both counts, and neither is a rounded-down estimate.
   *
   * The route asks this of the *destination* chain's binding, to fill the
   * executor's `GasInstruction` for the delivery. Delivering on Stellar is a
   * Soroban `receive_message`, which has no `msg.value` to attach and no
   * caller-set gas limit: the relayer's cost is the resource fee its own
   * simulation computes.
   *
   * Measured on the localnet, that fee is not one number. A first delivery of a
   * plain transfer simulated at 270_527 stroops (11.8M instructions) against a
   * one-guardian set, and at 358_889 stroops (41.6M instructions) against a
   * 19-guardian set signed to mainnet's quorum of 13 — the signatures the core
   * recovers dominate the CPU budget, so the figure moves with the guardian set
   * that verifies the delivery. The network's own fee configuration then prices
   * both, and a localnet only approximates it.
   *
   * Solana answers with a real `msgValue` because its destination-side rent and
   * signature costs are protocol constants; Soroban's is a simulation result,
   * and what turns this into a number is simulating `receive_message` against
   * the destination at quote time rather than a constant measured elsewhere.
   * Nothing reaches this in any case: the route rejects a destination whose
   * executor capabilities do not list `ERN1`, and no quoter prices 61.
   */
  async estimateMsgValueAndGasLimit(
    _recipient: ChainAddress | undefined
  ): Promise<{ msgValue: bigint; gasLimit: bigint }> {
    return { msgValue: 0n, gasLimit: 0n };
  }

  /**
   * Resolves the executor's `payee` from the 32 raw bytes the signed quote
   * carries at `quote[24..56]`.
   *
   * The executor binds those bytes to `payee.to_payload()` and accepts
   * **either** an `AccountIdPublicKeyEd25519` or a `ContractIdHash`, so it is
   * no help in telling them apart: both readings of the same bytes pass its
   * check, and only one of them is where the relayer's fee actually lands. The
   * bytes are a contract id if a contract instance exists at them —
   * nothing else plausibly explains one being there — and an account otherwise.
   *
   * The remaining wrong turn is loud rather than silent: an account that does
   * not exist cannot receive a credit, so the transfer fails in simulation
   * instead of paying into a balance nobody holds the key to.
   */
  private async payee(payeeAddress: Uint8Array): Promise<string> {
    if (payeeAddress.length !== 32)
      throw new Error(
        `Expected a 32-byte quote payee, got ${payeeAddress.length} bytes`
      );

    const contract = StrKey.encodeContract(Buffer.from(payeeAddress));
    const { entries } = await this.provider.getLedgerEntries(
      new Contract(contract).getFootprint()
    );
    return entries.length > 0
      ? contract
      : StrKey.encodeEd25519PublicKey(Buffer.from(payeeAddress));
  }

  /** Returns the wrapper to invoke, resolving it only where one is actually needed. */
  private wrapper(): string {
    const address =
      this.wrapperAddress ?? STELLAR_ADDRESSES[this.network]?.nttWithExecutor;
    if (address === undefined)
      throw new Error(
        `No ntt-with-executor deployment for ${this.chain} on ${this.network}`
      );
    return address;
  }
}

/** `FeeArgs.dbps` is `u32` on the ABI but must fit the on-wire `u16` fee field. */
const MAX_U16 = 0xffffn;
