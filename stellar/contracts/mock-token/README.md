# mock-token

A minimal fungible token used as a fixture by the NTT tests. It is not for production. It implements only the part of the Soroban token interface the manager exercises, plus a permissionless `mint` so tests can fund accounts without an admin-key ceremony.

Package `mock-token`, `#![no_std]`, builds as a `cdylib`. Depends only on `soroban-sdk`.

## Interface

| Method                         | Auth     | Behavior                                         |
| ------------------------------ | -------- | ------------------------------------------------ |
| `__constructor(decimals: u32)` | deploy   | stores the decimal precision in instance storage |
| `decimals() -> u32`            | none     | returns the configured precision                 |
| `balance(id) -> i128`          | none     | returns the balance, or 0 if unset               |
| `mint(to, amount)`             | **none** | credits `to`; test-only                          |
| `burn(from, amount)`           | `from`   | debits `from`, panics on insufficient balance    |
| `transfer(from, to, amount)`   | `from`   | moves `amount`, panics on insufficient balance   |

Balances live in persistent storage keyed by `Address`; decimals live in instance storage. There is no allowance, `transfer_from`, `burn_from`, name, symbol, or admin. This is a subset, not a full [SEP-41](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md) token and not a Stellar Asset Contract.

## Why a custom token

How the manager calls the token in burning mode dictates the choice:

- Burning **outbound** calls `burn(from, amount)`, authorized by the holder. A stock Stellar Asset Contract (SAC) supports this.
- Burning **inbound** calls `mint(to, amount)`. On a real SAC, `mint` is an admin-only operation, so using burning mode with a SAC would require making the manager contract the token administrator.

The mock sidesteps that setup by making `mint` unauthenticated. The manager can mint on inbound without being an admin, and tests can pre-fund senders directly. Parameterized decimals also let the tests exercise the manager's decimal trimming (for example a 9-decimal token trimming to the 8-decimal wire domain).

Locking mode needs none of this, since it only calls `transfer`. The tests use the real native XLM Stellar Asset Contract there.

A production NTT deployment should bridge a real SAC or a hardened SEP-41 token that gates `mint` to the manager. See [`integration-tests`](../../integration-tests) for how the mock is deployed and driven.
