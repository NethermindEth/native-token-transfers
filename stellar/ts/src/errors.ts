/**
 * Naming for the contract errors a failed NTT call reports.
 *
 * Every write is simulated before it is yielded, so a rejection arrives as a
 * host error string carrying `Error(Contract, #N)` rather than a typed value.
 * `N` only means something together with the contract that produced it: the
 * manager and the transceiver number their own errors independently
 * (`soroban_ntt_client::errors`), while the OpenZeppelin-derived pause and
 * ownership codes are shared by both and sit far above either range.
 */

/** Which contract's error vocabulary a code should be read against. */
export type ContractErrorSpace = "NttManager" | "Transceiver" | "WormholeCore";

const NTT_MANAGER_ERRORS: Record<number, string> = {
  1: "MessageTooShort",
  2: "InvalidPrefix",
  3: "InvalidDecimals",
  4: "ChainIdTooLarge",
  5: "PayloadTooLong",
  6: "DecimalMismatch",
  7: "AmountOverflow",
  8: "InvalidChainIdZero",
  13: "NotAdminOrPauser",
  20: "RateLimitNotInitialized",
  30: "NotInitialized",
  40: "TransceiverNotRegistered",
  41: "MaxTransceiversReached",
  42: "ZeroThreshold",
  43: "ThresholdTooHigh",
  44: "TransceiverAlreadyEnabled",
  45: "TransceiverAlreadyDisabled",
  46: "CannotDisableLastTransceiver",
  47: "NoEnabledTransceivers",
  48: "BitmapIndexOutOfRange",
  49: "TransceiverCallFailed",
  50: "PeerNotFound",
  51: "InvalidPeerChainIdZero",
  52: "InvalidPeerSameChainId",
  53: "InvalidPeerZeroAddress",
  54: "InvalidPeerDecimals",
  55: "InvalidPeer",
  60: "ZeroAmount",
  61: "InvalidRecipient",
  62: "TransferExceedsRateLimit",
  63: "TransferNotQueued",
  64: "TransferNotReleasable",
  65: "CancellerNotSender",
  66: "RecipientNotRegistered",
  67: "WormholeCoreCallFailed",
  80: "TransceiverNotEnabled",
  81: "TransceiverAlreadyAttested",
  82: "TransferAlreadyRedeemed",
  83: "InvalidTargetChain",
  84: "TransferNotApproved",
};

const TRANSCEIVER_ERRORS: Record<number, string> = {
  1: "NotInitialized",
  10: "InvalidPeerChainIdZero",
  11: "InvalidPeerZeroAddress",
  12: "PeerAlreadySet",
  13: "PeerNotFound",
  14: "PeerDisabled",
  15: "ChainIdTooLarge",
  20: "WormholeVerificationFailed",
  21: "WormholeQueryFailed",
  22: "WormholePostFailed",
  23: "ManagerQueryFailed",
  30: "InvalidTransceiverPrefix",
  31: "MessageTooShort",
  32: "PayloadTooLong",
  33: "UnexpectedRecipientManager",
  34: "ReplayDetected",
  35: "UnexpectedEmitter",
  36: "ManagerRejectedMessage",
};

/**
 * The two codes a caller has to act on rather than just read: `receive_message`
 * reports every manager failure as the first, and only the inner frames say
 * whether the cause was the second. Kept beside the tables they come from.
 */
export const MANAGER_REJECTED_MESSAGE = 36;
export const RECIPIENT_NOT_REGISTERED = 66;

/**
 * `stellar_contract_utils::pausable::PausableError` and
 * `stellar_access::{ownable::OwnableError, role_transfer::RoleTransferError}`.
 * Both contracts derive their pause and ownership gates from these, so the
 * codes mean the same thing whichever one raised them.
 */
const OZ_ERRORS: Record<number, string> = {
  1000: "EnforcedPause",
  1001: "ExpectedPause",
  2100: "OwnerNotSet",
  2101: "TransferInProgress",
  2102: "OwnerAlreadySet",
  2200: "NoPendingTransfer",
  2201: "InvalidLiveUntilLedger",
  2202: "InvalidPendingAccount",
  2203: "TransferExpired",
};

/**
 * `soroban_env_host::builtin_contracts::ContractError`, raised by the built-in
 * Stellar Asset Contract. Every NTT write that moves tokens has a token frame
 * in it, and the host reports these in the same `Error(Contract, #N)` form as a
 * contract's own, over a range that collides with the tables above.
 */
const TOKEN_ERRORS: Record<number, string> = {
  1: "InternalError",
  2: "OperationNotSupported",
  3: "AlreadyInitialized",
  4: "Unauthorized",
  5: "AuthenticationError",
  6: "AccountMissing",
  7: "AccountIsNotClassic",
  8: "NegativeAmount",
  9: "AllowanceError",
  10: "BalanceError",
  11: "BalanceDeauthorized",
  12: "Overflow",
  13: "TrustlineMissing",
};

/**
 * The vocabularies each space's codes may come from, most specific first.
 *
 * A code is only meaningful together with the contract that raised it, and a
 * host error does not say which frame that was, so a code the tables disagree
 * about is reported as every candidate rather than as a confident guess.
 */
const SPACES: Record<ContractErrorSpace, Record<number, string>[]> = {
  // The manager custodies through the built-in token contract, so its writes
  // carry a token frame.
  NttManager: [NTT_MANAGER_ERRORS, OZ_ERRORS, TOKEN_ERRORS],
  // No token frame surfaces here: everything below `receive_message` is masked
  // as `ManagerRejectedMessage (36)`.
  Transceiver: [TRANSCEIVER_ERRORS, OZ_ERRORS],
  // The Wormhole core's own `WormholeError` vocabulary lives in the Wormhole
  // repo, not here, and its registry writes move no tokens. Its codes are
  // reported as-is rather than misread against a table they do not belong to.
  WormholeCore: [OZ_ERRORS],
};

/** Every name `code` could have in `space`, or `"Unknown"` if it has none. */
const errorName = (code: number, space: ContractErrorSpace): string =>
  SPACES[space].flatMap((table) => table[code] ?? []).join(" | ") || "Unknown";

/**
 * The contract error codes a host error mentions, outermost first.
 *
 * A simulation failure quotes the frame that failed and then its diagnostic
 * events, so a call that failed inside a sub-invocation reports the outer code
 * first and the inner one further along.
 */
export function contractErrorCodes(error: unknown): number[] {
  const message = error instanceof Error ? error.message : String(error);
  return [...message.matchAll(/Error\(Contract, #(\d+)\)/g)].flatMap((m) =>
    m[1] === undefined ? [] : [Number(m[1])]
  );
}

/**
 * Restates a failed call with the contract error it carries named, or returns
 * it unchanged when it carries none (an RPC or network failure, say).
 */
export function decodeContractError(
  error: unknown,
  space: ContractErrorSpace
): Error {
  const cause = error instanceof Error ? error : new Error(String(error));
  const [code] = contractErrorCodes(error);
  if (code === undefined) return cause;

  return new Error(
    `${space}Error::${errorName(code, space)} (${code}): ${cause.message}`,
    { cause }
  );
}
