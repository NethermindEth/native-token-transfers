import { nativeToScVal, rpc as SorobanRpc } from "@stellar/stellar-sdk";
import type { Network } from "@wormhole-foundation/sdk-base";
import type { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import {
  StellarAddress,
  StellarPlatform,
} from "@wormhole-foundation/sdk-stellar";
import {
  ADDRESS_NOT_FOUND,
  contractErrorCodes,
  decodeContractError,
} from "./errors.js";

/**
 * Signals that the core registry holds no address for a hash.
 *
 * Callers distinguish an unregistered address, which
 * `StellarNtt.recordAddress` fixes, from a simulation or network failure,
 * which it does not.
 */
export class AddressNotFoundError extends Error {
  constructor(hash: UniversalAddress, options?: ErrorOptions) {
    super(`No address recorded for hash ${hash.toString()}`, options);
    this.name = "AddressNotFoundError";
  }
}

/**
 * Resolves a `hash_address` back to the typed `G…`/`C…` address it was
 * computed from.
 *
 * `StellarAddress.toUniversalAddress()` is the forward direction —
 * `keccak256(strkey text)`, the identity every NTT message carries on the wire
 * — and it is one-way. The Wormhole core's address registry is the only
 * inverse, so this throws `AddressNotFoundError` unless
 * `StellarNtt.recordAddress` registered that address beforehand.
 */
export async function resolveAddress(
  rpc: SorobanRpc.Server,
  network: Network,
  coreAddress: string,
  hash: UniversalAddress
): Promise<StellarAddress> {
  let resolved: unknown;
  try {
    resolved = await StellarPlatform.simulateRead(
      rpc,
      network,
      coreAddress,
      "get_address_from_hash",
      nativeToScVal(Buffer.from(hash.toUint8Array()), { type: "bytes" })
    );
  } catch (e) {
    // The outermost frame, the same one `decodeContractError` names. A
    // deeper frame carrying the code came from some other call, so it falls
    // through and gets decoded rather than reported as a missing address.
    const [code] = contractErrorCodes(e);
    if (code === ADDRESS_NOT_FOUND)
      throw new AddressNotFoundError(hash, { cause: e });
    throw decodeContractError(e, "WormholeCore");
  }
  if (typeof resolved !== "string")
    throw new Error(`Unexpected get_address_from_hash result: ${resolved}`);
  return new StellarAddress(resolved);
}
