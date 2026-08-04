import {
  Address,
  BASE_FEE,
  Contract,
  TransactionBuilder,
  nativeToScVal,
  rpc as SorobanRpc,
} from "@stellar/stellar-sdk";
import type { Network } from "@wormhole-foundation/sdk-base";
import type { UniversalAddress } from "@wormhole-foundation/sdk-definitions";
import {
  StellarAddress,
  StellarPlatform,
  StellarUnsignedTransaction,
  stellarNetworkPassphrase,
} from "@wormhole-foundation/sdk-stellar";
import type {
  AnyStellarAddress,
  StellarChains,
} from "@wormhole-foundation/sdk-stellar";

/**
 * The on-chain reverse of `hash_address`.
 *
 * `StellarAddress.toUniversalAddress()` is the forward direction —
 * `keccak256(strkey text)`, the identity every NTT message carries on the wire
 * — and it is one-way. A typed `G…`/`C…` is recovered only from the Wormhole
 * core's address registry, which has to be populated first, so the two halves
 * live here together.
 */

/**
 * Registers `address` on the core address registry, so its hash can be
 * resolved by {@link resolveAddress}.
 *
 * Permissionless: `payer` only funds the transaction and need not be the
 * address being recorded. Inbound NTT transfers to an unregistered recipient
 * fail (`RecipientNotRegistered`) until this lands, and the entry is persistent
 * storage, so a long-idle registration can need restoring.
 */
export async function* recordAddress<
  N extends Network,
  C extends StellarChains,
>(
  rpc: SorobanRpc.Server,
  network: N,
  chain: C,
  coreAddress: string,
  payer: AnyStellarAddress,
  address: AnyStellarAddress
): AsyncGenerator<StellarUnsignedTransaction<N, C>> {
  const from = new StellarAddress(payer).toString();
  const source = await rpc.getAccount(from);
  const tx = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: stellarNetworkPassphrase(network),
  })
    .addOperation(
      new Contract(coreAddress).call(
        "record_address",
        new Address(new StellarAddress(address).toString()).toScVal()
      )
    )
    .setTimeout(30)
    .build();

  yield new StellarUnsignedTransaction(
    await rpc.prepareTransaction(tx),
    network,
    chain,
    "Ntt.recordAddress"
  );
}

/**
 * Resolves a `hash_address` back to the typed `G…`/`C…` address it was
 * computed from.
 *
 * Throws `AddressNotFound` unless {@link recordAddress} registered that address
 * beforehand — the hash carries no hint of which kind of address produced it,
 * so the registry is the only inverse.
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
