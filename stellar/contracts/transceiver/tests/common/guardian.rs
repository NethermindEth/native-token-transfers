/// Secret key of the single test guardian the suite signs VAAs with. The
/// in-process Wormhole core is seeded with the matching address (see
/// [`guardian_eth_address`]) so those signatures verify.
pub const GUARDIAN_SECRET: [u8; 32] = [0x42; 32];

/// The 20-byte Ethereum-style address the Wormhole core must hold in its
/// guardian set for [`GUARDIAN_SECRET`]'s signatures to verify.
pub fn guardian_eth_address() -> [u8; 20] {
    vaa_test_signing::eth_address_from_privkey(&GUARDIAN_SECRET)
}
