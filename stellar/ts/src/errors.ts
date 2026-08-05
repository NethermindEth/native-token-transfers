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
export type ContractErrorSpace = "NttManager" | "Transceiver";

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
 * The contract error codes a host error mentions, outermost first.
 *
 * A simulation failure quotes the frame that failed and then its diagnostic
 * events, so a call that failed inside a sub-invocation reports the outer code
 * first and the inner one further along.
 */
export function contractErrorCodes(error: unknown): number[] {
  const message = error instanceof Error ? error.message : String(error);
  return [...message.matchAll(/Error\(Contract, #(\d+)\)/g)].map((m) =>
    Number(m[1])
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

  const errors =
    space === "NttManager" ? NTT_MANAGER_ERRORS : TRANSCEIVER_ERRORS;
  const name = errors[code] ?? OZ_ERRORS[code] ?? "Unknown";
  return new Error(`${space}Error::${name} (${code}): ${cause.message}`, {
    cause,
  });
}
