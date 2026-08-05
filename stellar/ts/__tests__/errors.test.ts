import { contractErrorCodes, decodeContractError } from "../src/errors.js";

// What `rpc.Server.prepareTransaction` rethrows when the simulation reverts:
// the failing frame, then the diagnostic events beneath it.
const simulationError = (...codes: number[]) =>
  new Error(
    [
      "host invocation failed",
      "",
      "Caused by:",
      ...codes.map((c) => `    HostError: Error(Contract, #${c})`),
    ].join("\n")
  );

describe("contract error decoding", () => {
  it("names a manager error", () => {
    expect(
      decodeContractError(simulationError(50), "NttManager").message
    ).toMatch(/NttManagerError::PeerNotFound \(50\)/);
  });

  it("reads the same code differently per contract", () => {
    // 13 is NotAdminOrPauser on the manager and PeerNotFound on the transceiver.
    expect(
      decodeContractError(simulationError(13), "NttManager").message
    ).toMatch(/NotAdminOrPauser/);
    expect(
      decodeContractError(simulationError(13), "Transceiver").message
    ).toMatch(/PeerNotFound/);
  });

  it("names the shared OpenZeppelin codes in either space", () => {
    expect(
      decodeContractError(simulationError(1000), "Transceiver").message
    ).toMatch(/EnforcedPause \(1000\)/);
    expect(
      decodeContractError(simulationError(2200), "NttManager").message
    ).toMatch(/NoPendingTransfer \(2200\)/);
  });

  it("keeps the original error when it names no contract code", () => {
    const raw = new Error("fetch failed");
    expect(decodeContractError(raw, "NttManager")).toBe(raw);
  });

  it("reports every code, outermost first", () => {
    // `flatten_call` masks the manager's own error as ManagerRejectedMessage,
    // so the cause is only recoverable from the inner frame.
    expect(contractErrorCodes(simulationError(36, 66))).toEqual([36, 66]);
    expect(contractErrorCodes(new Error("fetch failed"))).toEqual([]);
  });
});
