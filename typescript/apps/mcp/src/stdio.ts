/**
 * Starts the Undrly MCP server on stdio (docs/v1.8-mcp.md §13).
 *
 * Environment, one of:
 * - DATABASE_URL: serve the API routes in this process over a read-only
 *   PostgreSQL pool (`default_transaction_read_only`), as `apps/api` does;
 * - UNDRLY_API_URL: read a running Undrly API over HTTP instead.
 * UNDRLY_STALE_AFTER_SECONDS (default 300) as for the API.
 *
 * stdout carries only MCP messages; diagnostics go to stderr.
 */
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import postgres from "postgres";
import { httpApi, inProcessApi, type UndrlyApi } from "./api.ts";
import { createUndrlyMcpServer } from "./server.ts";

const apiUrl = process.env["UNDRLY_API_URL"];
const databaseUrl = process.env["DATABASE_URL"];

let api: UndrlyApi;
let sql: postgres.Sql | null = null;
if (apiUrl !== undefined && apiUrl !== "") {
  api = httpApi(apiUrl);
} else if (databaseUrl !== undefined && databaseUrl !== "") {
  sql = postgres(databaseUrl, {
    max: 3,
    connection: { default_transaction_read_only: true },
    onnotice: () => {},
  });
  api = inProcessApi(sql, {
    staleAfterSeconds: Number(process.env["UNDRLY_STALE_AFTER_SECONDS"] ?? 300),
  });
} else {
  console.error("undrly-mcp: set DATABASE_URL (in process) or UNDRLY_API_URL (over HTTP)");
  process.exit(1);
}

// One factory serves both protocol eras: a 2025-11-25 `initialize` opening
// and a 2026-07-28 `server/discover` one. Each instance is stateless.
const handle = serveStdio(() => createUndrlyMcpServer(api), {
  onerror: (e) => console.error("undrly-mcp:", e.message),
});
const shutdown = async () => {
  await handle.close();
  await sql?.end({ timeout: 2 });
  process.exit(0);
};
process.stdin.on("end", shutdown);
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
console.error(`undrly-mcp: ready on stdio (${apiUrl ? `API ${apiUrl}` : "in process"})`);
