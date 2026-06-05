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
LAST_SEQ=0
ADVANCES=0
while [ "$ADVANCES" -lt 2 ]; do
  printf "."
  HEALTH=$(curl -sf "$SOROBAN_RPC_URL" -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":{}}' || echo "")
  if echo "$HEALTH" | grep -q '"status":"healthy"'; then
    SEQ=$(curl -sf "$SOROBAN_RPC_URL" -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger","params":{}}' 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('sequence', 0))" 2>/dev/null || echo 0)
    if [ "${SEQ:-0}" -gt "$LAST_SEQ" ]; then
      ADVANCES=$((ADVANCES + 1))
      LAST_SEQ=$SEQ
    fi
  fi
  sleep 3
done
echo
echo "Localnet ready (ledger=$LAST_SEQ)."
