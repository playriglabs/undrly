#!/usr/bin/env bash
# Runs a script against a database in the local `undrly-hack-pg` container,
# reading the container's password (URL-encoded) so it never has to be typed:
#
#   ./scripts/local-db.sh dev.sh                 # API + collector on undrly_v11
#   ./scripts/local-db.sh history.sh             # backfill undrly_v11
#   UNDRLY_DB=undrly_v12 ./scripts/local-db.sh dev.sh
#
# Container, port and database default to undrly-hack-pg, 55432, undrly_v11.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
container="${UNDRLY_DB_CONTAINER:-undrly-hack-pg}"
port="${UNDRLY_DB_PORT:-55432}"
db="${UNDRLY_DB:-undrly_v11}"

command -v jq >/dev/null || { echo "local-db.sh: jq is required" >&2; exit 1; }
password="$(docker inspect "$container" --format '{{range .Config.Env}}{{println .}}{{end}}' \
  | sed -n 's/^POSTGRES_PASSWORD=//p')"
if [ -z "$password" ]; then
  echo "local-db.sh: no POSTGRES_PASSWORD in container $container (is it running?)" >&2
  exit 1
fi
export DATABASE_URL="postgres://undrly:$(jq -rn --arg p "$password" '$p | @uri')@127.0.0.1:${port}/${db}"
echo "==> database: 127.0.0.1:${port}/${db} (container ${container})"

script="${1:?usage: local-db.sh <dev.sh|history.sh|demo.sh> [args…]}"
shift
exec "$root/scripts/${script}" "$@"
