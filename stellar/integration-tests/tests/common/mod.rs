//! Shared fixtures for the integration suite: the "standard peer" constants
//! that nearly every scenario registers, and a builder for a valid inbound
//! VAA from that peer which tests perturb via struct-update syntax.

use integration_tests::messages::{
    stellar_addr_to_bytes32, InboundVaaInputs, NttManagerMessageInputs,
};
use integration_tests::TestContext;

/// Wormhole chain id of the standard test peer.
pub const PEER_CHAIN: u32 = 2;
/// Emitter / manager address of the standard test peer.
pub const PEER_ADDR: [u8; 32] = [0xaa; 32];
/// Token decimals the standard peer advertises.
pub const PEER_DECIMALS: u32 = 8;

/// The default inbound transfer amount, in 8-decimal wire units. Scenarios
/// with amount-specific logic (rate-limit primer/queued sets, decimal
/// trimming) define their own.
pub const STANDARD_TRIMMED_AMOUNT: u64 = 100_000_000;

/// What the recipient receives for [`STANDARD_TRIMMED_AMOUNT`] under the
/// default token's 7 decimals: the 8-decimal wire amount untrimmed to 7
/// decimals (`100_000_000 / 10`). Both burning (mint) and locking (unlock)
/// land on this value.
pub const STANDARD_RECEIVED: i128 = 10_000_000;

/// A throwaway recipient for scenarios that never read the recipient's
/// balance (outbound transfers, inbound rejections). Tests that assert a
/// minted or unlocked balance use a funded `setup_identity` instead.
pub const DUMMY_RECIPIENT: [u8; 32] = [0x40; 32];

/// A valid inbound VAA from the standard peer ([`PEER_ADDR`] on
/// [`PEER_CHAIN`]) addressed to `manager`, sending `trimmed_amount` 8-decimal
/// wire units to `recipient` at wormhole `sequence`. Tests perturb individual
/// fields (emitter, recipient_manager, …) via struct-update syntax.
pub fn peer_inbound_vaa<'a>(
    ctx: &'a TestContext,
    manager: &str,
    recipient: [u8; 32],
    trimmed_amount: u64,
    sequence: u64,
) -> InboundVaaInputs<'a> {
    InboundVaaInputs {
        ntt: NttManagerMessageInputs {
            id: [0x10; 32],
            sender: [0x20; 32],
            source_token: [0x30; 32],
            recipient,
            recipient_chain: ctx.stellar_chain_id,
            trimmed_amount,
            trimmed_decimals: 8,
        },
        source_manager: PEER_ADDR,
        recipient_manager: stellar_addr_to_bytes32(manager),
        emitter_chain: PEER_CHAIN as u16,
        emitter_address: PEER_ADDR,
        sequence,
        guardian_secret: &ctx.guardian_secret,
    }
}
