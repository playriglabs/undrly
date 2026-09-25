#!/usr/bin/env bash
# Cross-market demo, end to end (docs/hackathon-v1.md):
#   migrate + seed → one sequential collection pass → start the read-only API
#   → run the demo checks → stop the API.
#
# Requires DATABASE_URL (a local demo database). For the NVDA (IEX) quote, also
# export APCA_API_KEY_ID and APCA_API_SECRET_KEY; without them that check fails.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${DATABASE_URL:?set DATABASE_URL to a local demo database}"
port="${PORT:-8787}"

(cd "$root/rust" && cargo build -q -p undrly-collect)
collect="$root/rust/target/debug/undrly-collect"
(cd "$root" && "$collect" seed)
(cd "$root" && "$collect" run --once) || echo "note: some sources failed this pass"

log="${TMPDIR:-/tmp}/undrly-api.log"
PORT="$port" bun "$root/typescript/apps/api/src/server.ts" >"$log" 2>&1 &
api=$!
trap 'kill $api 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:$port/health" >/dev/null && break
  sleep 0.1
done
API_URL="http://127.0.0.1:$port" bun run "$root/scripts/demo.ts"
