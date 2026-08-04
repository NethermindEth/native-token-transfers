import { encoding } from "@wormhole-foundation/sdk-base";
import { StellarAddress } from "@wormhole-foundation/sdk-stellar";

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

describe("hash_address", () => {
  it.each(HASH_ADDRESS_VECTORS)(
    "matches the contract hash for $strkey",
    ({ strkey, hash }) => {
      const universal = new StellarAddress(strkey).toUniversalAddress();
      expect(encoding.hex.encode(universal.toUint8Array())).toEqual(hash);
    }
  );
});
