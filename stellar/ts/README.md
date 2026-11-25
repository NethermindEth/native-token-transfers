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

The RPC sets the resource fee from its simulation, with no cap of its own. `maxFee` caps the whole fee, in stroops, and defaults to 50,000,000 (5 XLM). It also caps, separately, the XLM a transfer pays its transceivers. To prepare a write that is known to cost more, raise it on the manager binding:

```ts
ntt.maxFee = 100_000_000n; // 10 XLM
```

The transceiver and executor bindings prepare their writes through the manager binding, so its `maxFee` applies to them too.

## Restore archived state

When a write touches persistent state that has been archived, the transaction restores that state before the write runs, and its resource fee pays for the restoration. If that pushes the fee above `maxFee`, the error says that restoring archived entries caused it. To fix it, restore those entries in a separate `restoreFootprint` transaction, and then prepare the write again.
