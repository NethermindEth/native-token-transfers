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

docker stop stellar_localnet >/dev/null 2>&1 || true

docker run --rm -d \
  -p 8000:8000 \
  --name stellar_localnet \
  stellar/quickstart:latest \
  --standalone

echo "Waiting for localnet to be ready..."
until curl -s "$SOROBAN_RPC_URL" -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":{}}' | grep -q '"status":"healthy"'; do
  printf "."
  sleep 2
done
echo
echo "Localnet ready."
