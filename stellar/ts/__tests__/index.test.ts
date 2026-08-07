import { getProtocolInitializer } from "@wormhole-foundation/sdk-definitions";
import { _platform } from "@wormhole-foundation/sdk-stellar";
import { StellarNtt, StellarNttWithExecutor } from "../src/index.js";

// `getProtocolInitializer` throws when nothing is registered, so importing the
// barrel is the whole test: it is the only thing that runs `registerProtocol`.
it("registers both protocols under the Stellar platform", () => {
  expect(getProtocolInitializer(_platform, "Ntt")).toBe(StellarNtt);
  expect(getProtocolInitializer(_platform, "NttWithExecutor")).toBe(
    StellarNttWithExecutor
  );
});
