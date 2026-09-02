import { registerProtocol } from "@wormhole-foundation/sdk-definitions";
import { _platform } from "@wormhole-foundation/sdk-stellar";
import { StellarNtt } from "./ntt.js";
import { StellarNttWithExecutor } from "./nttWithExecutor.js";
import "@wormhole-foundation/sdk-definitions-ntt";

registerProtocol(_platform, "Ntt", StellarNtt);
registerProtocol(_platform, "NttWithExecutor", StellarNttWithExecutor);

export * from "./address.js";
export * from "./constants.js";
export * from "./errors.js";
export * from "./messages.js";
export * from "./ntt.js";
export * from "./nttWithExecutor.js";
export * from "./scval-types.js";
export * from "./transceiver.js";
