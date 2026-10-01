#!/usr/bin/env bash
# Landing page (SvelteKit, landing/) in one command.
#
#   ./scripts/landing.sh           # dev server on http://127.0.0.1:${PORT:-5173}
#   ./scripts/landing.sh build     # static build into landing/build
#   ./scripts/landing.sh preview   # build, then serve landing/build on ${PORT:-4173}
#
# Installs dependencies when landing/node_modules is missing. Reads
# landing/.env when present (VITE_SITE_URL sets the social preview origin).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="$root/landing"
mode="${1:-dev}"

command -v bun >/dev/null || { echo "landing.sh: bun is required" >&2; exit 1; }

if [ ! -d "$dir/node_modules" ]; then
  echo "==> installing landing dependencies"
  (cd "$dir" && bun install --frozen-lockfile)
fi

case "$mode" in
  dev)
    port="${PORT:-5173}"
    echo "==> landing dev server on http://127.0.0.1:$port"
    cd "$dir" && exec bun run dev -- --port "$port"
    ;;
  build)
    echo "==> building landing"
    cd "$dir" && exec bun run build
    ;;
  preview)
    port="${PORT:-4173}"
    echo "==> building landing"
    (cd "$dir" && bun run build)
    echo "==> serving landing/build on http://127.0.0.1:$port"
    cd "$dir" && exec bun run preview -- --port "$port"
    ;;
  *)
    echo "usage: landing.sh [dev|build|preview]" >&2
    exit 1
    ;;
esac
