# vaa-test-signing

Host-side guardian primitives for producing signed Wormhole VAAs in tests. In production, VAAs are signed off-chain by the Wormhole guardian network. This crate simulates a single guardian so the tests can submit valid signed VAAs to a real on-chain Wormhole core and exercise its signature verification, both in the in-process unit suite and against localnet in [`integration-tests`](../integration-tests).

This is a plain host crate (not a contract). It does not serialize the VAA body itself; the caller does that with `wormhole_soroban_client::VAA::serialize_body`. This crate signs the body hash and assembles the envelope around it. Dependencies are `secp256k1` (recoverable signatures) and `tiny-keccak`.

## API

```rust
struct GuardianSignature { index: u8, sig: [u8; 64], recovery_id: u8 }

fn keccak256(data: &[u8]) -> [u8; 32]
fn eth_address_from_privkey(privkey: &[u8; 32]) -> [u8; 20]
fn sign(body: &[u8], privkey: &[u8; 32], guardian_index: u8) -> GuardianSignature
fn assemble(guardian_set_index: u32, sigs: &[GuardianSignature], body: &[u8]) -> Vec<u8>
```

- `eth_address_from_privkey` derives the 20-byte Ethereum-style address that identifies a guardian on the wire (`keccak256(uncompressed_pubkey[1..])[12..]`). Tests seed the Wormhole core's guardian set with this address so a signature from the matching key verifies.
- `sign` follows the guardian convention: it signs `keccak256(keccak256(body))` with a recoverable secp256k1 signature and records the recovery id.
- `assemble` lays out the envelope the core accepts on `parse_and_verify_vaa`: a version byte `1`, the guardian-set index (4 bytes, big-endian), the signature count, each signature as `index(1) ‖ sig(64) ‖ recovery_id(1)`, then the body.

## How the tests use it

The integration harness seeds the core with a one-guardian set at index 0, derived from a committed localnet-only secret. To build an inbound VAA, a test serializes the layered body (NTT manager message, then transceiver message, then VAA body), calls `sign(body, secret, 0)`, and wraps it with `assemble(0, [sig], body)`. Because the on-chain core holds the matching guardian address, the VAA verifies. A tampered-signature variant flips one byte so verification fails with `WormholeVerificationFailed`, the only way to drive real secp256k1 rejection end to end (the unit suite mocks the core).

The test guardian secret is checked in on purpose. It is localnet-only and must never be reused on testnet or mainnet.
