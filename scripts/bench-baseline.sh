#!/usr/bin/env bash
# Run store microbenchmarks and save a log under target/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LOG="${ROOT}/target/bench-store-$(date +%Y%m%d-%H%M%S).log"
mkdir -p "$(dirname "$LOG")"

echo "Logging to: $LOG"
exec > >(tee "$LOG") 2>&1

echo "SYNTHESIST_BENCH_CLAIMS=${SYNTHESIST_BENCH_CLAIMS:-<unset>}"
echo "SYNTHESIST_DIR=${SYNTHESIST_DIR:-<unset>}"
echo

cargo bench -p nomograph-synthesist --bench store "$@"
