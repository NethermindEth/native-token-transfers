// ESM Jest does not inject the `jest` global; import it.
import { jest } from "@jest/globals";
import { encoding } from "@wormhole-foundation/sdk-base";
import {
  StellarAddress,
  StellarPlatform,
} from "@wormhole-foundation/sdk-stellar";
import { scValToNative, type rpc as SorobanRpc } from "@stellar/stellar-sdk";
import { AddressNotFoundError, resolveAddress } from "../src/address.js";

// Vectors emitted by the contracts' own `hash_address`
// (`wormhole_soroban_client::hash_address`, pinned to keccak256 of the StrKey
// text by stellar/contracts/manager/tests/hash_address_vectors.rs and used as
// `stellar_addr_to_hash` in stellar/integration-tests/src/messages.rs).
// If the shared primitive ever changes its hash input, this fails loudly.
const HASH_ADDRESS_VECTORS = [
  {
    strkey: "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF",
    hash: "9ada4dcf333bbfbee08664c291c9c588c324ae1c37ac6389999a2f3b6e1c1610",
  },
  {
    strkey: "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4",
    hash: "0c5b3171908de4562a52491f3281bc3c4490b9a5498ffebf692e1184f6f3192b",
  },
] as const;

const CORE = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
const rpc = {} as SorobanRpc.Server;

/** Stands in for a simulation rejection carrying `code`. */
const contractError = (code: number) =>
  new Error(`HostError: Error(Contract, #${code})`);

const resolve = (hash: StellarAddress) =>
  resolveAddress(rpc, "Testnet", CORE, hash.toUniversalAddress());

afterEach(() => jest.restoreAllMocks());

describe("hash_address", () => {
  it.each(HASH_ADDRESS_VECTORS)(
    "matches the contract hash for $strkey",
    ({ strkey, hash }) => {
      const universal = new StellarAddress(strkey).toUniversalAddress();
      expect(encoding.hex.encode(universal.toUint8Array())).toEqual(hash);
    }
  );
});

describe("resolveAddress", () => {
  it.each(HASH_ADDRESS_VECTORS)(
    "returns the address the registry holds for $strkey",
    async ({ strkey, hash }) => {
      const read = jest
        .spyOn(StellarPlatform, "simulateRead")
        .mockResolvedValue(strkey);

      // The round trip the on-chain registry exists to invert: the forward
      // hash is one-way, so only `get_address_from_hash` recovers the strkey.
      const resolved = await resolve(new StellarAddress(strkey));
      expect(resolved.toString()).toEqual(strkey);

      // The method name and the hash argument are part of the ABI, so a
      // rename or a wrong encoding has to fail here rather than pass on the
      // stub's say-so.
      const [, , contract, method, arg] = read.mock.calls[0]!;
      expect([contract, method]).toEqual([CORE, "get_address_from_hash"]);
      expect(scValToNative(arg!)).toEqual(Buffer.from(hash, "hex"));
    }
  );

  it("reports an unrecorded hash as AddressNotFoundError", async () => {
    jest
      .spyOn(StellarPlatform, "simulateRead")
      .mockRejectedValue(contractError(43));

    await expect(
      resolve(new StellarAddress(HASH_ADDRESS_VECTORS[0].strkey))
    ).rejects.toThrow(AddressNotFoundError);
  });

  it("keeps an unrelated failure decoded rather than not-found", async () => {
    jest
      .spyOn(StellarPlatform, "simulateRead")
      .mockRejectedValue(contractError(1000));

    const thrown = await resolve(
      new StellarAddress(HASH_ADDRESS_VECTORS[0].strkey)
    ).catch((e: unknown) => e);

    expect(thrown).not.toBeInstanceOf(AddressNotFoundError);
    expect(thrown).toHaveProperty(
      "message",
      expect.stringContaining("EnforcedPause")
    );
  });

  it("leaves a deeper frame carrying the code to the decoder", async () => {
    // Only the outermost frame names the call that failed. A 43 raised
    // further in came from some other contract and is not a missing address.
    jest
      .spyOn(StellarPlatform, "simulateRead")
      .mockRejectedValue(
        new Error("HostError: Error(Contract, #1000), Error(Contract, #43)")
      );

    await expect(
      resolve(new StellarAddress(HASH_ADDRESS_VECTORS[0].strkey))
    ).rejects.not.toBeInstanceOf(AddressNotFoundError);
  });

  it("rejects a result that is not a strkey", async () => {
    jest.spyOn(StellarPlatform, "simulateRead").mockResolvedValue(null);

    await expect(
      resolve(new StellarAddress(HASH_ADDRESS_VECTORS[0].strkey))
    ).rejects.toThrow(/Unexpected get_address_from_hash result/);
  });
});
