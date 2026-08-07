#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STELLAR_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$STELLAR_DIR"

# `stellar contract build --optimize` runs wasm-opt only on freshly compiled
# output. If cargo considers the crates up to date and a prior plain build left
# an unoptimized binary in place, the oversized manager wasm survives and the
# protocol-25 network rejects it on upload (TxSorobanInvalid). Remove the
# artifacts first so the optimized wasm the tests upload is always regenerated.
rm -f target/wasm32v1-none/release/stellar_ntt_manager.wasm \
  target/wasm32v1-none/release/stellar_ntt_transceiver.wasm \
  target/wasm32v1-none/release/stellar_ntt_with_executor.wasm \
  target/wasm32v1-none/release/mock_token.wasm
stellar contract build --optimize

for wasm in \
  "target/wasm32v1-none/release/stellar_ntt_manager.wasm" \
  "target/wasm32v1-none/release/stellar_ntt_transceiver.wasm" \
  "target/wasm32v1-none/release/stellar_ntt_with_executor.wasm" \
  "target/wasm32v1-none/release/mock_token.wasm" \
  "integration-tests/vendor/wormhole_core.wasm" \
  "integration-tests/vendor/wormhole_executors.wasm"; do
  if [ ! -f "$wasm" ]; then
    echo "ERROR: missing artifact: $wasm" >&2
    exit 1
  fi
done

echo "All WASM artifacts present."
