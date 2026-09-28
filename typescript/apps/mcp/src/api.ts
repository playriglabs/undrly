/**
 * The one way the MCP server reads Undrly: the read-only HTTP API's own
 * routes (`apps/api/src/app.ts`), either in process over a read-only
 * PostgreSQL pool or over HTTP against a running API. Both run the same
 * resolver, explain, graph and market-data code; the MCP package has no SQL
 * and no second resolver (docs/v1.8-mcp.md §2).
 */
import { createApp } from "@undrly/api/app";
import type postgres from "postgres";

/** An API response: HTTP status and parsed JSON body. */
export type ApiResponse = { status: number; body: unknown };

/** `GET <path>` against the Undrly API (`path` starts with `/`). */
export type UndrlyApi = (path: string) => Promise<ApiResponse>;

/** The API routes in this process, over `sql` (which must be read-only). */
export function inProcessApi(
  sql: postgres.Sql,
  options: { staleAfterSeconds: number; now?: () => Date },
): UndrlyApi {
  const app = createApp(sql, options);
  return async (path) => {
    const res = await app.request(path);
    return { status: res.status, body: await res.json() };
  };
}

/** A running Undrly API at `baseUrl` (e.g. `http://127.0.0.1:8787`). */
export function httpApi(baseUrl: string): UndrlyApi {
  const base = baseUrl.replace(/\/+$/, "");
  return async (path) => {
    const res = await fetch(`${base}${path}`, { headers: { accept: "application/json" } });
    return { status: res.status, body: await res.json() };
  };
}
