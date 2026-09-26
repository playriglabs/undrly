#!/usr/bin/env bash
# V1.3 market-data backfill (docs/v1.3-market-data.md): venue bars, reference
# series history and the equity trading calendar, into the same database the
# API reads. Run it beside `./scripts/dev.sh`; the API serves new data as it
# lands, no restart.
#
#   ./scripts/history.sh                          # everything (~25 min)
#   ./scripts/history.sh bars --days-1h 2 --days-1d 7   # refresh recent bars
#   ./scripts/history.sh reference                # reference series only (~1 min)
#
# Uses DATABASE_URL when set (e.g. the one you give dev.sh); otherwise the
# compose PostgreSQL from .env, like dev.sh. Alpaca keys (equity bars,
# calendar) and EIA_API_KEY (WTI/Brent/Henry Hub history) come from .env;
# without them those parts are skipped. Safe to stop with Ctrl-C and re-run:
# everything already fetched stays, nothing is duplicated.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

if [ -f "$root/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$root/.env"
  set +a
fi

if [ -z "${DATABASE_URL:-}" ]; then
  command -v jq >/dev/null || { echo "history.sh: jq is required" >&2; exit 1; }
  : "${UNDRLY_POSTGRES_PASSWORD:?set UNDRLY_POSTGRES_PASSWORD in .env, or export DATABASE_URL}"
  password="$(jq -rn --arg p "$UNDRLY_POSTGRES_PASSWORD" '$p | @uri')"
  export DATABASE_URL="postgres://undrly:${password}@127.0.0.1:5432/undrly"
fi

echo "==> building the collector"
(cd "$root/rust" && cargo build -q -p undrly-collect)

echo "==> backfilling into ${DATABASE_URL##*@} (${*:-all})"
cd "$root"
"$root/rust/target/debug/undrly-collect" history "${@:-all}"

port="${PORT:-8787}"
cat <<EOF

Backfill done. With ./scripts/dev.sh running, try:

  http://127.0.0.1:$port/v1/candles/BTC/USD?interval=1h&limit=5
  http://127.0.0.1:$port/v1/candles/EUR/USD?interval=1d&limit=5
  http://127.0.0.1:$port/v1/candles/BTC-PERP?interval=4h&limit=5
  http://127.0.0.1:$port/v1/market/AAPL
  http://127.0.0.1:$port/v1/derivatives/BTC-PERP
  http://127.0.0.1:$port/v1/history/USD/IDR?limit=5
EOF
