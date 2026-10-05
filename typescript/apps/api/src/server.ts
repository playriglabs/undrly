/**
 * Starts the API. Environment: DATABASE_URL (required), PORT (default 8787),
 * UNDRLY_STALE_AFTER_SECONDS (default 300).
 *
 * Every connection is read-only (`default_transaction_read_only`); the API
 * never writes and never contacts an upstream source.
 */
import postgres from "postgres";
import { createApp } from "./app.ts";
import { warmMarketSummaries } from "./market-data.ts";

const url = process.env["DATABASE_URL"];
if (url === undefined) {
  console.error("DATABASE_URL is not set");
  process.exit(1);
}
const sql = postgres(url, {
  max: 5,
  connection: { default_transaction_read_only: true },
});
const staleAfterSeconds = Number(process.env["UNDRLY_STALE_AFTER_SECONDS"] ?? 300);
const app = createApp(sql, { staleAfterSeconds });
// `/v1/markets?sort=` orders by values computed for every market (V1.10).
warmMarketSummaries(sql, staleAfterSeconds);
const port = Number(process.env["PORT"] ?? 8787);
console.log(`undrly api listening on http://127.0.0.1:${port}`);

// Bun serves a default export with `fetch` (run with `bun run src/server.ts`).
export default { port, fetch: app.fetch };
