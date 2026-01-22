#!/bin/bash
set -e

# Environment
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env.localnet"

if [ -f "$ENV_FILE" ]; then
  set -a
  source "$ENV_FILE"
  set +a
else
  echo "Environment file $ENV_FILE not found"
  exit 1
fi

cd "$SCRIPT_DIR/../.."
PROJECT_ROOT=$(pwd)

# Convert to absolute path
if [[ "$NTT_MANAGER_WASM_PATH" != /* ]]; then
  export NTT_MANAGER_WASM_PATH="$PROJECT_ROOT/$NTT_MANAGER_WASM_PATH"
fi
if [[ "$NTT_TRANSCEIVER_WASM_PATH" != /* ]]; then
  export NTT_TRANSCEIVER_WASM_PATH="$PROJECT_ROOT/$NTT_TRANSCEIVER_WASM_PATH"
fi
if [[ "$MOCK_TOKEN_WASM_PATH" != /* ]]; then
  export MOCK_TOKEN_WASM_PATH="$PROJECT_ROOT/$MOCK_TOKEN_WASM_PATH"
fi

echo "Building contracts using stellar contract build..."
stellar contract build

echo "Running integration tests..."
# localnet tests are ignored with cargo test
cargo test -p integration-tests -- --ignored --nocapture --test-threads=1
