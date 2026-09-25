import { v1 } from "@undrly/contracts";
import type postgres from "postgres";
import { describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";

// `GET /` touches no database; any use of `sql` would throw.
const noDatabase = new Proxy(() => {}, {
  get: () => {
    throw new Error("GET / must not query the database");
  },
  apply: () => {
    throw new Error("GET / must not query the database");
  },
}) as unknown as postgres.Sql;

describe("GET /", () => {
  it("describes the service, its endpoints and examples", async () => {
    const res = await createApp(noDatabase, { staleAfterSeconds: 300 }).request("/");
    expect(res.status).toBe(200);
    const index = v1.ServiceIndexV1.parse(await res.json());
    expect(index.description).toBe("One normalized API across every market.");
    expect(index.examples).toContain("/v1/quote/BTC/USD");
    expect(index.endpoints.map((e) => e.path)).toContain("/v1/quotes/{query}");
    expect(index.dataUse).toMatch(/unreviewed/);
  });
});
