#!/bin/bash
set -e

docker stop stellar_localnet >/dev/null 2>&1 || true

if docker ps -a --format '{{.Names}}' | grep -q '^stellar_localnet$'; then
  docker rm -f stellar_localnet >/dev/null 2>&1 || true
fi

echo "Localnet stopped."
