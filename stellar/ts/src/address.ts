import { nativeToScVal, rpc as SorobanRpc } from "@stellar/stellar-sdk";
import type { Network } from "@wormhole-foundation/sdk-base";
import type { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import {
  StellarAddress,
  StellarPlatform,
} from "@wormhole-foundation/sdk-stellar";

/**
 * Resolves a `hash_address` back to the typed `G…`/`C…` address it was
 * computed from.
 *
 * `StellarAddress.toUniversalAddress()` is the forward direction —
 * `keccak256(strkey text)`, the identity every NTT message carries on the wire
 * — and it is one-way. The Wormhole core's address registry is the only
 * inverse, so this throws `AddressNotFound` unless
 * `StellarNtt.recordAddress` registered that address beforehand.
 */
export async function resolveAddress(
  rpc: SorobanRpc.Server,
  network: Network,
  coreAddress: string,
  hash: UniversalAddress
): Promise<StellarAddress> {
  const resolved = await StellarPlatform.simulateRead(
    rpc,
    network,
    coreAddress,
    "get_address_from_hash",
    nativeToScVal(Buffer.from(hash.toUint8Array()), { type: "bytes" })
  );
  if (typeof resolved !== "string")
    throw new Error(`Unexpected get_address_from_hash result: ${resolved}`);
  return new StellarAddress(resolved);
}
