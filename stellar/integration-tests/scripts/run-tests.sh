#!/bin/bash
set -e

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

STELLAR_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$STELLAR_DIR"

for var in NTT_MANAGER_WASM_PATH NTT_TRANSCEIVER_WASM_PATH MOCK_TOKEN_WASM_PATH WORMHOLE_CORE_WASM_PATH; do
  val="${!var}"
  if [[ "$val" != /* ]]; then
    export "$var"="$STELLAR_DIR/$val"
  fi
done

"$SCRIPT_DIR/build-wasms.sh"

cargo test -p integration-tests -- --ignored --nocapture --test-threads=1
