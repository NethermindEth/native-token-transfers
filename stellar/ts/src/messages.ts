import { deserializeLayout } from "@wormhole-foundation/sdk-base";
import type { Ntt } from "@wormhole-foundation/sdk-definitions-ntt";
import {
  nativeTokenTransferLayout,
  nttManagerMessageLayout,
} from "@wormhole-foundation/sdk-definitions-ntt";

/**
 * Reconstructs the `NttManagerMessage` a transceiver hands to the manager.
 *
 * `manager_payload` is the opaque inner field of a `TransceiverMessage`
 * (`soroban_ntt_client::messages`); the Soroban encoding is byte-identical to
 * the shared NTT wire format, so the `sdk-definitions-ntt` layouts decode it
 * unchanged.
 *
 * The digest the manager keys attestations and queue entries by is
 * `Ntt.messageDigest(sourceChain, message)` — `keccak256(be_u16(chain) ‖
 * message)`, matching `NttManagerMessage::compute_digest`. It needs no RPC, so
 * do not read it back off-chain.
 */
export function parseNttManagerMessage(
  managerPayload: Uint8Array
): Ntt.Message {
  return deserializeLayout(
    nttManagerMessageLayout(nativeTokenTransferLayout),
    managerPayload
  );
}
