import { encoding, serializeLayout } from "@wormhole-foundation/sdk-base";
import {
  Ntt,
  nativeTokenTransferLayout,
  nttManagerMessageLayout,
} from "@wormhole-foundation/sdk-definitions-ntt";
import { StellarAddress } from "@wormhole-foundation/sdk-stellar";
import { parseNttManagerMessage } from "../src/messages.js";

// Produced by the contracts' own encoders — `NttManagerMessage::to_bytes` and
// `::compute_digest(source_chain = 61)` from soroban-ntt-client, the same ones
// stellar/integration-tests/src/messages.rs drives — over a message whose
// sender is hash_address(G...) and whose source token is hash_address(C...).
const ACCOUNT = "GA5KWLHVHDUXW4YUM7A5MFEJ3CDNN4C3Z3T3VGG2DQUWIZMJSWIN56CF";
const CONTRACT = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
const MANAGER_MESSAGE =
  "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f" + // id
  "9ada4dcf333bbfbee08664c291c9c588c324ae1c37ac6389999a2f3b6e1c1610" + // sender
  "004f" + // payload length
  "994e5454" + // NTT prefix
  "07" + // decimals
  "00000000075bcd15" + // trimmed amount
  "0c5b3171908de4562a52491f3281bc3c4490b9a5498ffebf692e1184f6f3192b" + // source token
  "fffefdfcfbfaf9f8f7f6f5f4f3f2f1f0efeeedecebeae9e8e7e6e5e4e3e2e1e0" + // recipient
  "0002"; // recipient chain
const DIGEST =
  "657f80a1331939f1c0ff94b1b42ec9b3ea1f16a2592a2047ddb279edabe0b6d8";

describe("NttManagerMessage", () => {
  const bytes = encoding.hex.decode(MANAGER_MESSAGE);
  const message = parseNttManagerMessage(bytes);

  it("decodes the Soroban wire format", () => {
    expect(message.sender).toEqual(
      new StellarAddress(ACCOUNT).toUniversalAddress()
    );
    expect(message.payload.sourceToken).toEqual(
      new StellarAddress(CONTRACT).toUniversalAddress()
    );
    expect(message.payload.trimmedAmount).toEqual({
      amount: 123456789n,
      decimals: 7,
    });
    expect(message.payload.recipientChain).toEqual("Ethereum");
    expect(message.payload.additionalPayload).toEqual(new Uint8Array());
  });

  it("round-trips back to the same bytes", () => {
    expect(
      serializeLayout(
        nttManagerMessageLayout(nativeTokenTransferLayout),
        message
      )
    ).toEqual(bytes);
  });

  it("computes the digest the manager keys attestations by", () => {
    expect(encoding.hex.encode(Ntt.messageDigest("Stellar", message))).toEqual(
      DIGEST
    );
  });
});
