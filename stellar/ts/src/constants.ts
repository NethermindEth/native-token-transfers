import { toChainId } from "@wormhole-foundation/sdk-base";
import type { Network } from "@wormhole-foundation/sdk-base";

/**
 * Per-network Stellar contract ids that are not carried in the chain's
 * `Contracts` config. The Wormhole core, manager and transceiver addresses are
 * config (injected by the caller / the localnet env); the `ntt-with-executor`
 * wrapper is not, because it is one deployment per chain serving every manager
 * on it — exactly as EVM's `nttManagerWithExecutorAddresses` and Sui's
 * `SUI_ADDRESSES` hold theirs. No Stellar NTT deployment is public yet, so this
 * table is empty until one exists.
 */
export const STELLAR_ADDRESSES: Partial<
  Record<Network, { nttWithExecutor: string }>
> = {};

/** Wormhole chain id for Stellar; the manager's `chain_id` is this value. */
export const STELLAR_CHAIN_ID = toChainId("Stellar");

/**
 * Default rate-limit refill window, mirroring
 * `soroban_ntt_client::constants::RATE_LIMIT_DURATION`. Soroban ledger time is
 * in **seconds** — unlike the millisecond durations the Sui/EVM SDKs use — so
 * every timestamp crossing this boundary is a second count.
 */
export const RATE_LIMIT_DURATION = 86400n;

/**
 * NTT wire prefixes as the big-endian u32s the contracts compare against
 * (`soroban_ntt_client::constants`). The sdk-definitions-ntt layouts already
 * embed these; they are exported for code that sniffs a raw payload's prefix
 * before choosing a layout.
 */
export const WIRE_PREFIXES = {
  ntt: 0x994e5454,
  wormholeTransceiver: 0x9945ff10,
  transceiverInfo: 0x9c23bd3b,
  transceiverRegistration: 0x18fc67c2,
} as const;
