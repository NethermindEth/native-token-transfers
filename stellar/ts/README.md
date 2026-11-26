# Stellar NTT SDK

`@wormhole-foundation/sdk-stellar-ntt` binds the Soroban NTT manager, the Wormhole transceiver, and the `ntt-with-executor` wrapper for the Wormhole TypeScript SDK. For the contracts themselves, see the [Stellar NTT README](../README.md).

## Transaction checks

Every write builds a transaction, simulates it, and yields it for the signer to sign and submit. The signer signs only the envelope, and that one signature authorizes every source-account entry in the auth tree the simulation recorded. `StellarNtt.prepare` builds every write in this package, so it checks the auth tree and the fee before it returns a transaction.

`prepare` rejects a transaction in the following cases:

- The simulation recorded an auth entry for an address other than the source account. The error names the address that would have to sign.
- The simulation recorded more than one auth entry, or an entry whose root is not the call that was built.
- The auth tree holds a call the write has no reason to make. Every write other than a transfer authorizes nothing below its own call. A transfer authorizes only the token's `transfer` or `burn` and native `transfer` calls, all from the sender, or from the payer of a queued transfer's completion. The token goes only to the manager, and XLM only to the configured Wormhole transceiver. The manager pays every enabled transceiver that quotes a fee, so a manager with more than one fee-charging transceiver cannot transfer through this binding.
- A transfer moves more of the token than its amount, which is zero for a queued transfer's completion, or pays its transceivers more XLM than `maxFee`. When the token is XLM, its custody counts against the amount. An executor transfer pays the executor at most the quote's estimated cost.
- An executor transfer authorizes anything but the referrer fee to the quote's referrer, one manager transfer, and `request_execution` on the executor, whose only calls pay the quote's payee in XLM. The executor is the one the wrapper was built with: `contracts.executor`, or else the network's `STELLAR_ADDRESSES` entry. The manager transfer has to carry the sender, the destination chain and recipient, and a `should_queue` of `false`, and with the referrer fee it has to add up to the amount. The referrer fee is at most the quote's `referrerFeeDbps` of the amount.
- The fee is above `maxFee`.

## Raise the fee cap

The RPC sets the resource fee from its simulation, with no cap of its own. `maxFee` caps the whole fee, in stroops, and defaults to 50,000,000 (5 XLM). It also caps, separately, the XLM a transfer pays its transceivers. The signer from `getStellarSigner` caps the fee too, at 5 XLM by default. To prepare and sign a write that is known to cost more, raise both caps:

```ts
ntt.maxFee = 100_000_000n; // 10 XLM
const signer = await getStellarSigner(rpc, secret, { maxFee: 100_000_000n });
```

The transceiver and executor bindings prepare their writes through the manager binding, so its `maxFee` applies to them too.

## Restore archived state

When a write touches persistent state that has been archived, the transaction restores that state before the write runs, and its resource fee pays for the restoration. If that pushes the fee above `maxFee`, the error says that restoring archived entries caused it. To fix it, restore those entries in a separate `restoreFootprint` transaction, signed with `@stellar/stellar-sdk` directly because `StellarSigner` signs only a transaction with one `invokeHostFunction` operation, and then prepare the write again.

## Redeem a transfer

The manager pays only a recipient recorded on the core address registry, because an NTT message carries the recipient's one-way `hash_address`. When a transfer names the payer of `redeem` as its recipient and the registry has no entry for it, `redeem` first yields a `recordAddress` transaction for the payer. Any other recipient, such as a `C…` contract or a transfer someone else redeems, has to be recorded first with `recordAddress`.

Submit each transaction `redeem` yields and wait until it lands before you resume the generator, as `signSendWait` does. A receive simulated before the record lands fails with the `RecipientNotRegistered` guidance.

## Read a transfer's sequence

A queued transfer is completed or cancelled by its sequence. `getTransferResult(hash)` reads the sequence, the queued flag and the digest from the confirmed transaction, given the hash that `signSendWait` or `signAndSend` returned. It reads the transaction's return value, so it works only while the RPC still holds the transaction.

## Replace a transceiver

The manager keeps a removed transceiver's registry index and disables it. A binding still configured with that transceiver fails to redeem with `TransceiverNotEnabled` (80), and `verifyAddresses` reports the enabled transceiver to configure instead.

A new transceiver has a new emitter, its raw contract id. It needs `set_peer` for every peer chain, and every peer chain has to register that id. A peer chain that registered the old transceiver can do that only with a new transceiver of its own, because a transceiver peer is set once on every chain. Upgrading the transceiver in place keeps its address and its peers and needs no config change, so prefer an upgrade.

A stale config also fails every transfer in `prepare` once the new transceiver charges a fee, because the binding pays XLM only to the configured transceiver. While both transceivers are enabled, a transfer through this binding fails in `prepare` if both charge a fee, as described in [Transaction checks](#transaction-checks).
