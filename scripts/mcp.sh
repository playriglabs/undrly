#!/usr/bin/env bash
# MCP verification (docs/v1.8-mcp.md §14): starts the Undrly MCP server on
# stdio as a child process and drives it with the official MCP client (both
# protocol eras): initialization, tool listing, resolve, explain, graph,
# quote, derivatives, the Solana, Robinhood Chain and Tempo journeys, and
# structured errors. No LLM and no upstream source is involved.
#
#   UNDRLY_DB=undrly_v14 ./scripts/local-db.sh mcp.sh
#
# Reads the database as it is: run worldsfair.sh (or dev.sh) on it first so
# identities and market data exist. The server's connections are read-only.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${DATABASE_URL:?set DATABASE_URL to a local demo database}"
[ -d "$root/typescript/node_modules" ] || (cd "$root/typescript" && bun install --frozen-lockfile >/dev/null)
exec bun "$root/typescript/apps/mcp/src/verify.ts"
