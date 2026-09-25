#!/usr/bin/env bash
# Terminal walkthrough for a live demo, against a running API.
#
#   ./scripts/present.sh          # all steps
#   ./scripts/present.sh --step   # pause before each step
#
# API_URL defaults to http://127.0.0.1:8787 (start it with scripts/dev.sh).
# Read-only: it only calls the Undrly API. scripts/demo.sh is the verifier.
set -euo pipefail
api="${API_URL:-http://127.0.0.1:8787}"
step_mode=false
[ "${1:-}" = "--step" ] && step_mode=true
command -v jq >/dev/null || { echo "present.sh: jq is required" >&2; exit 1; }
curl -sf "$api/health" >/dev/null || { echo "present.sh: no API at $api (run ./scripts/dev.sh)" >&2; exit 1; }

if [ -t 1 ]; then
  b=$'\e[1m' d=$'\e[2m' c=$'\e[36m' g=$'\e[32m' y=$'\e[33m' r=$'\e[0m'
else
  b='' d='' c='' g='' y='' r=''
fi

header() { # n title route
  if $step_mode; then read -r -p "${d}(enter)${r} " _ </dev/tty; fi
  printf '\n%s[%s/3] %s%s   %s%s%s\n\n' "$b" "$1" "$2" "$r" "$d" "$3" "$r"
}
get() { curl -s "$api$1"; }
fresh() { [ "$1" = "fresh" ] && printf '%s%s%s' "$g" "$1" "$r" || printf '%s%s%s' "$y" "$1" "$r"; }

printf '\n%sUndrly — one normalized API across every market%s\n' "$b" "$r"
printf '%scollect → normalize → aggregate → serve%s\n' "$d" "$r"

# 1. Five markets, one API ----------------------------------------------------------
header 1 "Five markets, one API" "GET /v1/quote/{query}"
printf "  ${b}%-12s %-9s %-22s %-10s %-11s %s${r}\n" MARKET QUERY PRICE TYPE BASIS FROM
freshness=""
for row in "equity NVDA" "crypto_spot BTC/USD" "fx EUR/USD" "commodity XAU/USD" "perpetual BTC-PERP"; do
  set -- $row
  set -- "${1/_/ }" "$2"
  body="$(get "/v1/quote/$2")"
  if ! echo "$body" | jq -e '.price' >/dev/null 2>&1; then
    printf "  %-12s %-9s %s\n" "$1" "$2" "$(echo "$body" | jq -r '"— " + (.error.code // "no answer")')"
    continue
  fi
  echo "$body" | jq -r --arg m "$1" --arg q "$2" '
    [ $m, $q, (.price + " " + (.unit.code // "?")), .priceType, .basis,
      (if .basis == "venue" then .venue.name + " (" + .source.id + ")"
       elif .source == null then ([.aggregation.inputs[].venue.name] | join(" + "))
       else .source.id end) ] | @tsv' |
    while IFS=$'\t' read -r m q price type basis from; do
      printf "  %-12s %-9s %-22s %-10s %-11s %s\n" "$m" "$q" "$price" "$type" "$basis" "$from"
    done
  freshness+="$2 $(fresh "$(echo "$body" | jq -r .freshness)")  "
done
printf '\n  %ssame request, same response shape, five asset classes%s\n' "$c" "$r"
printf '  %sfreshness: %s%s\n' "$d" "$freshness" "$r"

# 2. BTC/USD: independent venue observations ------------------------------------------
header 2 "BTC/USD: two independent venues" "GET /v1/quotes/BTC/USD"
quotes="$(get /v1/quotes/BTC/USD)"
printf "  ${b}%-18s %-13s %-13s %-20s %s${r}\n" VENUE BID ASK PRICE "AS OF"
echo "$quotes" | jq -r '.observations[] |
  [ (.venue.name // .source.id), (.bid // "-"), (.ask // "-"), (.priceType + " " + .price),
    ((.observedAt // .receivedAt)[11:19] + (if .observedAt then " venue time" else " received" end)),
    .freshness ] | @tsv' |
  while IFS=$'\t' read -r venue bid ask price asof fr; do
    printf "  %-18s %-13s %-13s %-20s %s  %s\n" "$venue" "$bid" "$ask" "$price" "$asof" "$(fresh "$fr")"
  done
printf '\n  %seach venue is kept as it reported, never overwritten%s\n' "$c" "$r"
echo "$quotes" | jq -r '.observations[] | "  provenance: \(.venue.name // .source.id) → stored response #\(.sourceRecord.id) \(.sourceRecord.key)"' |
  while IFS= read -r line; do printf '%s%s%s\n' "$d" "$line" "$r"; done

# 3. One canonical aggregated quote -----------------------------------------------------
header 3 "One canonical quote" "GET /v1/quote/BTC/USD"
ms="$(curl -s -o /dev/null -w '%{time_total}' "$api/v1/quote/BTC/USD" | awk '{printf "%.0f", $1 * 1000}')"
quote="$(get /v1/quote/BTC/USD)"
if echo "$quote" | jq -e '.price' >/dev/null 2>&1; then
  echo "$quote" | jq -r '"  \(.price) \(.unit.code)   \(.priceType) · basis \(.basis) · no single venue"' |
    while IFS= read -r line; do printf '%s%s%s\n' "$b" "$line" "$r"; done
  echo "$quote" | jq -r '
    "  method   \(.aggregation.method) over \(.aggregation.eligibleObservations) fresh venue(s)",
    "  inputs   " + ([.aggregation.inputs[] | "\(.venue.name) \(.price)"] | join("  +  ")) + "  → mean of mids",
    "  as of    \(.asOf)  (\(.freshness)), computed \(.aggregation.computedAt[11:19])"'
else
  printf '  %s\n' "$(echo "$quote" | jq -r '.error.message // "no quote"')"
fi
printf '\n  %sanswered in %s ms from Undrly'"'"'s store; upstream sources are polled in the background, never per request%s\n\n' "$c" "$ms" "$r"
