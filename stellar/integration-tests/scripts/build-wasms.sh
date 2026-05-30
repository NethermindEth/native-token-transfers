#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STELLAR_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$STELLAR_DIR"
stellar contract build --optimize

for wasm in \
  "target/wasm32v1-none/release/stellar_ntt_manager.wasm" \
  "target/wasm32v1-none/release/stellar_ntt_transceiver.wasm" \
  "target/wasm32v1-none/release/mock_token.wasm" \
  "integration-tests/vendor/wormhole_core.wasm"; do
  if [ ! -f "$wasm" ]; then
    echo "ERROR: missing artifact: $wasm" >&2
    exit 1
  fi
done

echo "All WASM artifacts present."
