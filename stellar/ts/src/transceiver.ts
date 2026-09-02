import { Contract, StrKey, nativeToScVal } from "@stellar/stellar-sdk";
import type { xdr } from "@stellar/stellar-sdk";
import { encoding } from "@wormhole-foundation/sdk-base";
import type { Chain, Network } from "@wormhole-foundation/sdk-base";
import {
  UniversalAddress,
  serialize,
} from "@wormhole-foundation/sdk-definitions";
import type {
  AccountAddress,
  ChainAddress,
} from "@wormhole-foundation/sdk-definitions";
import type {
  StellarNttTransceiver,
  WormholeNttTransceiver,
} from "@wormhole-foundation/sdk-definitions-ntt";
import { StellarPlatform } from "@wormhole-foundation/sdk-stellar";
import type {
  StellarChains,
  StellarUnsignedTransaction,
} from "@wormhole-foundation/sdk-stellar";
import {
  MANAGER_REJECTED_MESSAGE,
  RECIPIENT_NOT_REGISTERED,
  contractErrorCodes,
} from "./errors.js";
import type { StellarNtt } from "./ntt.js";
import {
  addressArg,
  asBoolean,
  asBytes,
  bytesArg,
  chainIdArg,
  decodePeerInfo,
  sequenceArg,
} from "./scval-types.js";

/**
 * The Wormhole transceiver contract that fronts a Soroban NTT manager.
 *
 * It posts outbound manager messages to the Wormhole core and verifies inbound
 * VAAs against it, so its own state is the peer registry (one emitter per
 * chain) and the set of VAAs it has already consumed.
 *
 * Reads simulate against the transceiver's own address; the manager binding
 * builds the writes, because it owns the package's only transaction builder,
 * and decodes them against the transceiver's error table rather than the
 * manager's.
 */
export class StellarNttWormholeTransceiver<
    N extends Network,
    C extends StellarChains,
  >
  implements
    WormholeNttTransceiver<N, C>,
    StellarNttTransceiver<N, C, WormholeNttTransceiver.VAA>
{
  constructor(
    readonly manager: StellarNtt<N, C>,
    readonly address: string
  ) {}

  /** Returns `b"wormhole"`, read off the contract rather than assumed. */
  async getTransceiverType(): Promise<string> {
    return encoding.bytes.decode(
      asBytes(await this.read("get_transceiver_type"))
    );
  }

  /**
   * Returns the 32 bytes a peer chain registers as this transceiver's emitter.
   *
   * That is the **raw contract id** — the core takes an emitter's identity
   * from `AddressPayload::ContractIdHash` and rejects anything else — and not
   * `hash_address`, the keccak256 of the StrKey text that
   * `StellarAddress.toUniversalAddress()` produces and that NTT messages carry
   * on the wire. The same contract has both identities; registering the hashed
   * one as this transceiver's peer emitter would fail every inbound VAA with
   * `TransceiverError::UnexpectedEmitter`.
   */
  async getAddress(): Promise<ChainAddress<C>> {
    return {
      chain: this.manager.chain,
      address: new UniversalAddress(
        new Uint8Array(StrKey.decodeContract(this.address))
      ),
    };
  }

  /**
   * Registers `peer` as this transceiver's counterpart on its chain, by the
   * emitter address its VAAs carry — for a Stellar peer, that address is what
   * {@link getAddress} returns. One-shot: a second call for the same chain
   * fails with `PeerAlreadySet`, so {@link setPeerEnabled} is the way to turn
   * a peer off and a redeploy the only way to correct one.
   */
  async *setPeer(
    peer: ChainAddress<Chain>,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.write(
      this.manager.source(payer, "the owner"),
      "Transceiver.setPeer",
      "set_peer",
      chainIdArg(peer.chain),
      bytesArg(peer.address.toUniversalAddress().toUint8Array())
    );
  }

  async getPeer<P extends Chain>(chain: P): Promise<ChainAddress<P> | null> {
    const emitter = await this.read("get_peer", chainIdArg(chain));
    // A peer emitter is the 32 raw wire bytes, which stay universal.
    return emitter === null
      ? null
      : {
          chain,
          address: new UniversalAddress(new Uint8Array(asBytes(emitter))),
        };
  }

  /**
   * Gets the peer's emitter and whether it is carrying messages, in one read.
   * A disabled peer keeps its emitter, so {@link getPeer} alone cannot tell a
   * live peer from a stopped one.
   */
  async getPeerInfo<P extends Chain>(
    chain: P
  ): Promise<{ address: ChainAddress<P>; enabled: boolean } | null> {
    const info = await this.read("get_peer_info", chainIdArg(chain));
    if (info === null) return null;

    const { emitter, enabled } = decodePeerInfo(info);
    return {
      address: {
        chain,
        address: new UniversalAddress(new Uint8Array(emitter)),
      },
      enabled,
    };
  }

  /**
   * Checks whether a peer is carrying messages; false for a chain with no peer
   * at all, as well as for a stopped one.
   */
  async isPeerEnabled(chain: Chain): Promise<boolean> {
    return asBoolean(await this.read("is_peer_enabled", chainIdArg(chain)));
  }

  /** Stops or resumes message flow with a peer {@link setPeer} registered. */
  async *setPeerEnabled(
    chain: Chain,
    enabled: boolean,
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.write(
      this.manager.source(payer, "the owner"),
      "Transceiver.setPeerEnabled",
      "set_peer_enabled",
      chainIdArg(chain),
      nativeToScVal(enabled, { type: "bool" })
    );
  }

  /**
   * Checks whether this VAA has already been delivered. The contract keys
   * replay protection by `(emitter chain, emitter address, sequence)` rather
   * than by the manager message digest, so this is what separates an
   * already-redeemed transfer from one {@link receive} would reject for some
   * other reason.
   */
  async isVaaConsumed(vaa: WormholeNttTransceiver.VAA): Promise<boolean> {
    return asBoolean(
      await this.read(
        "is_vaa_consumed",
        chainIdArg(vaa.emitterChain),
        bytesArg(vaa.emitterAddress.toUint8Array()),
        sequenceArg(vaa.sequence)
      )
    );
  }

  /**
   * Returns `null`: the transceiver has no pauser role at all, and pausing is
   * owner-only.
   */
  async getPauser(): Promise<AccountAddress<C> | null> {
    return null;
  }

  // Takes the interface's arguments and ignores them, so that holding this
  // class concretely is never narrower than holding the interface.
  async *setPauser(
    _newPauser: AccountAddress<C>,
    _payer?: AccountAddress<C>
  ): AsyncGenerator<never> {
    throw new Error(
      "The Stellar transceiver has no pauser role: pause is owner-only"
    );
  }

  /**
   * Pauses the transceiver as an emergency stop, owner-only — unlike the
   * manager's, which the pauser can also call. The contract ignores the caller
   * argument and enforces owner auth instead, but the argument is still part of
   * its ABI.
   */
  async *pause(
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.manager.source(payer, "the owner");
    yield await this.write(
      from,
      "Transceiver.pause",
      "pause",
      addressArg(from)
    );
  }

  /** Resumes the transceiver. Owner-only, like {@link pause}. */
  async *unpause(
    payer?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    const from = this.manager.source(payer, "the owner");
    yield await this.write(
      from,
      "Transceiver.unpause",
      "unpause",
      addressArg(from)
    );
  }

  /**
   * Verifies `attestation` against the Wormhole core and forwards its manager
   * message on. Permissionless: `sender` only sources and funds the
   * transaction, and the tokens go to the recipient the message names.
   */
  async *receive(
    attestation: WormholeNttTransceiver.VAA,
    sender?: AccountAddress<C>
  ): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
    yield await this.write(
      this.manager.source(sender, "the payer"),
      "Transceiver.receive",
      "receive_message",
      bytesArg(serialize(attestation))
    ).catch((error: unknown) => {
      // `prepare` has already named the transceiver error; this only adds what
      // the masked inner failure means for the caller.
      throw redeemGuidance(error);
    });
  }

  /** Simulates a read-only transceiver call and returns its decoded result. */
  private read(method: string, ...args: xdr.ScVal[]): Promise<unknown> {
    return StellarPlatform.simulateRead(
      this.manager.provider,
      this.manager.network,
      this.address,
      method,
      ...args
    );
  }

  /**
   * Prepares a transceiver call sourced from `from`, using the manager's
   * transaction builder and naming errors against the transceiver's own error
   * table.
   */
  private write(
    from: string,
    description: string,
    method: string,
    ...args: xdr.ScVal[]
  ): Promise<StellarUnsignedTransaction<N, C>> {
    return this.manager.prepare(
      from,
      new Contract(this.address).call(method, ...args),
      description,
      "Transceiver"
    );
  }
}

/**
 * Explains a `ManagerRejectedMessage` rejection and passes anything else
 * through as decoded.
 *
 * `receive_message` forwards to the manager through `flatten_call`, which
 * collapses every manager failure into `ManagerRejectedMessage` — the real
 * cause survives only in the simulation's inner frames. The retryable one is
 * the recipient never having been recorded on the core address registry, so
 * this names it and says what to do about it.
 */
const redeemGuidance = (error: unknown): unknown => {
  const codes = contractErrorCodes(error);
  if (!codes.includes(MANAGER_REJECTED_MESSAGE)) return error;

  const masked = codes.includes(RECIPIENT_NOT_REGISTERED)
    ? "The recipient is not registered (NttManagerError::RecipientNotRegistered, 66)."
    : "The manager rejected the message and flatten_call masked its error.";
  return new Error(
    `${error instanceof Error ? error.message : String(error)}\n${masked} ` +
      `If the recipient has never been recorded, run ` +
      `StellarNtt.recordAddress(payer, recipient) and redeem again.`,
    { cause: error }
  );
};
