#!/usr/bin/env bash
# Local Undrly in one command: PostgreSQL + collector + read-only API.
#
#   cp .env.example .env    # set UNDRLY_POSTGRES_PASSWORD (+ Alpaca keys for NVDA)
#   ./scripts/dev.sh        # API on http://127.0.0.1:${PORT:-8787}; Ctrl-C stops everything
#
# Uses the compose PostgreSQL unless DATABASE_URL is already set. The collector
# runs in the background (log: $TMPDIR/undrly-collector.log) and the API in the
# foreground. Upstream data is for local/private demo use only
# (docs/sources/quotes.md).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
port="${PORT:-8787}"
tmp="${TMPDIR:-/tmp}"
collector_log="${tmp%/}/undrly-collector.log"

need() { command -v "$1" >/dev/null || { echo "dev.sh: $1 is required" >&2; exit 1; }; }
need cargo
need bun
need jq

if [ -f "$root/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$root/.env"
  set +a
fi

if [ -z "${DATABASE_URL:-}" ]; then
  need docker
  : "${UNDRLY_POSTGRES_PASSWORD:?set UNDRLY_POSTGRES_PASSWORD in .env (cp .env.example .env)}"
  echo "==> starting PostgreSQL (docker compose)"
  if ! out="$(cd "$root" && docker compose up -d postgres 2>&1)"; then
    echo "$out" >&2
    exit 1
  fi
  for _ in $(seq 1 60); do
    (cd "$root" && docker compose exec -T postgres pg_isready -U undrly -d undrly >/dev/null 2>&1) && break
    sleep 1
  done
  password="$(jq -rn --arg p "$UNDRLY_POSTGRES_PASSWORD" '$p | @uri')"
  export DATABASE_URL="postgres://undrly:${password}@127.0.0.1:5432/undrly"
fi

if [ -z "${APCA_API_KEY_ID:-}" ] || [ -z "${APCA_API_SECRET_KEY:-}" ]; then
  echo "note: no Alpaca keys; NVDA will have no quote (the other four markets work)"
fi

echo "==> building the collector"
(cd "$root/rust" && cargo build -q -p undrly-collect)
collect="$root/rust/target/debug/undrly-collect"
[ -d "$root/typescript/node_modules" ] || (cd "$root/typescript" && bun install --frozen-lockfile >/dev/null)

echo "==> seeding reference data"
(cd "$root" && "$collect" seed)

echo "==> collector running in the background (log: $collector_log)"
(cd "$root" && exec "$collect" run) >"$collector_log" 2>&1 &
collector=$!
trap 'kill "$collector" 2>/dev/null || true' EXIT INT TERM
sleep 3 # first polling pass

cat <<EOF

Undrly is up: http://127.0.0.1:$port   (Ctrl-C to stop)

  curl -s localhost:$port/ | jq
  curl -s localhost:$port/v1/quote/BTC/USD | jq
  curl -s localhost:$port/v1/quotes/BTC/USD | jq
  curl -s localhost:$port/v1/quote/NVDA | jq
  curl -s "localhost:$port/v1/search?q=gold" | jq

  Market data (V1.3; fill it with ./scripts/history.sh in another terminal):
  curl -s "localhost:$port/v1/candles/BTC/USD?interval=1h&limit=5" | jq
  curl -s localhost:$port/v1/market/AAPL | jq
  curl -s localhost:$port/v1/derivatives/BTC-PERP | jq
  curl -s "localhost:$port/v1/history/USD/IDR?limit=5" | jq
  curl -s localhost:$port/v1/calendar/AAPL | jq

  ./scripts/present.sh --step

EOF
PORT="$port" bun "$root/typescript/apps/api/src/server.ts"
