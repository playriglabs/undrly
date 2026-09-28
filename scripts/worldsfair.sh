#!/usr/bin/env bash
# World's Fair verification (docs/v1.5-solana.md §15), end to end over
# production data:
#   migrate + seed → one collection pass → onchain identities (Solana chain
#   from its genesis hash, Circle's USDC deployments) → start the read-only
#   API → run the checks → stop the API.
#
# Requires DATABASE_URL. Perpetual candles need `scripts/history.sh` (bars)
# to have run on the same database. Ecosystems without an authoritative
# integration report NOT_CONFIGURED; they never pass by default.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${DATABASE_URL:?set DATABASE_URL to a local demo database}"
port="${PORT:-8787}"

(cd "$root/rust" && cargo build -q -p undrly-collect)
collect="$root/rust/target/debug/undrly-collect"
(cd "$root" && "$collect" seed)
(cd "$root" && "$collect" run --once) || echo "note: some sources failed this pass"
(cd "$root" && "$collect" onchain)

log="${TMPDIR:-/tmp}/undrly-api-worldsfair.log"
PORT="$port" bun "$root/typescript/apps/api/src/server.ts" >"$log" 2>&1 &
api=$!
trap 'kill $api 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:$port/health" >/dev/null && break
  sleep 0.1
done
API_URL="http://127.0.0.1:$port" bun run "$root/scripts/worldsfair.ts"
