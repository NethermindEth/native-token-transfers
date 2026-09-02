import { contractErrorCodes, decodeContractError } from "../src/errors.js";

// The shape `rpc.Server.prepareTransaction` rethrows verbatim from the
// simulation's `error` field: the frame that failed, then its diagnostic event
// log. Both put the outermost failure first, which is what `[0]` relies on.
const simulationError = (...codes: number[]) =>
  new Error(
    [
      "host invocation failed",
      "",
      "Caused by:",
      `    HostError: Error(Contract, #${codes[0]})`,
      "    ",
      "    Event log (newest first):",
      ...codes.map(
        (c, i) =>
          `       ${i}: [Diagnostic] topics:[error, Error(Contract, #${c})]`
      ),
    ].join("\n")
  );

describe("contract error decoding", () => {
  it("names a manager error", () => {
    expect(
      decodeContractError(simulationError(50), "NttManager").message
    ).toMatch(/NttManagerError::PeerNotFound \(50\)/);
  });

  it("reads the same code differently per contract", () => {
    // 13 is `NotAdminOrPauser` on the manager and `PeerNotFound` on the
    // transceiver.
    expect(
      decodeContractError(simulationError(13), "NttManager").message
    ).toMatch(/NotAdminOrPauser/);
    expect(
      decodeContractError(simulationError(13), "Transceiver").message
    ).toMatch(/PeerNotFound/);
  });

  it("names the token codes the manager's own table shadows", () => {
    // The manager custodies with the built-in token contract, whose errors
    // arrive in the same form; 13 is its `TrustlineMissing` as much as the
    // manager's `NotAdminOrPauser`, and naming one of them is a guess.
    expect(
      decodeContractError(simulationError(13), "NttManager").message
    ).toMatch(/NotAdminOrPauser \| TrustlineMissing \(13\)/);
    // The transceiver reaches no token: a manager failure below it is masked.
    expect(
      decodeContractError(simulationError(13), "Transceiver").message
    ).toMatch(/TransceiverError::PeerNotFound \(13\)/);
    // Unambiguous, and unnamed before: running out of XLM to pay a fee.
    expect(
      decodeContractError(simulationError(10), "NttManager").message
    ).toMatch(/BalanceError \(10\)/);
  });

  it("reads a wrapper transfer against all four of its frames", () => {
    // One `ntt_with_executor::transfer` runs wrapper, manager, executor, and
    // token frames, and the host error says only the number.
    const named = (code: number) =>
      decodeContractError(simulationError(code), "NttWithExecutor").message;

    expect(named(2)).toMatch(
      /NttWithExecutorError::PeerNotFound \| InvalidPrefix \| OperationNotSupported \(2\)/
    );
    // Unique on this path: the executor's, the manager's, and the token's.
    expect(named(16)).toMatch(/::QuotePayeeMismatch \(16\)/);
    expect(named(62)).toMatch(/::TransferExceedsRateLimit \(62\)/);
    expect(named(10)).toMatch(/::BalanceError \(10\)/);
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
    // `flatten_call` masks the manager's own error as
    // `ManagerRejectedMessage`, so the cause is only recoverable from the
    // inner frame.
    expect(contractErrorCodes(simulationError(36, 66))).toEqual([36, 36, 66]);
    expect(
      decodeContractError(simulationError(36, 66), "Transceiver").message
    ).toMatch(/ManagerRejectedMessage \(36\)/);
    expect(contractErrorCodes(new Error("fetch failed"))).toEqual([]);
  });
});
