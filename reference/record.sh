#!/usr/bin/env bash
# Record fixtures from the original core: reference/record.sh [scenarios/foo.json ...]
set -euo pipefail
cd "$(dirname "$0")/.."
python3 reference/prepare.py
docker volume create sbrw-m2 >/dev/null
status=0
docker compose -f reference/compose.yaml run --rm recorder "$@" || status=$?
docker compose -f reference/compose.yaml down --volumes --remove-orphans >/dev/null 2>&1 || true
exit $status
