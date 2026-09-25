/**
 * V1.1 API additions against a real PostgreSQL database (synthetic data):
 * `/v1/universes`, freshness by feed cadence (EIA 14 days, World Bank 62
 * days), additive subject fields, pair disambiguation by quote feeds, venue
 * aliases (`NYSE:` = `XNYS:`) and class-share punctuation (`EXB-B` = `EXB.B`).
 * `fetch` is stubbed to fail: the API never contacts an upstream provider.
 *
 * Skipped without DATABASE_URL (the role needs CREATEDB).
 */
import { readdirSync, readFileSync } from "node:fs";
import { v1 } from "@undrly/contracts";
import postgres from "postgres";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";

const url = process.env["DATABASE_URL"];
const ROOT = new URL("../../../../", import.meta.url);

const U = {
  usd: "0192f000-0000-7000-8000-000000000001",
  wti: "0192f000-0000-7000-8000-000000000002",
  maize: "0192f000-0000-7000-8000-000000000003",
  coin: "0192f000-0000-7000-8000-000000000004",
  stock: "0192f000-0000-7000-8000-000000000005",
  perp: "0192f000-0000-7000-8000-000000000006",
  classB: "0192f000-0000-7000-8000-000000000007",
  xnys: "0192f000-0000-7000-8000-000000000008",
  listing: "0192f000-0000-7000-8000-000000000009",
};

describe.skipIf(url === undefined)("V1.1 API with a database", () => {
  const name = `undrly_api_v11_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  let spyRecord = "";

  beforeAll(async () => {
    admin = postgres(url as string, { max: 1, onnotice: () => {} });
    await admin.unsafe(`CREATE DATABASE "${name}"`);
    const target = new URL(url as string);
    target.pathname = `/${name}`;
    sql = postgres(target.toString(), { max: 2, onnotice: () => {} });
    const dir = new URL("database/migrations/", ROOT);
    for (const file of readdirSync(dir).sort()) {
      await sql.unsafe(readFileSync(new URL(file, dir), "utf8")).simple();
    }
    await sql`INSERT INTO sources (id, name) VALUES ('undrly-curated', 'Curated'),
      ('undrly-universe', 'Snapshot'), ('eia', 'EIA'), ('worldbank', 'World Bank'),
      ('ssga', 'SSGA'), ('kraken', 'Kraken')`;
    const record = async (source: string, key: string, at: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${key}, ${Buffer.from(key)}, ${at}) RETURNING id::text`
      )[0]?.id ?? "";
    const at = "2026-09-01T00:00:00Z";
    const curated = await record("undrly-curated", "commodities.json", at);
    await sql`INSERT INTO nodes (id, category) VALUES (${U.usd}, 'currency'),
      (${U.wti}, 'instrument'), (${U.maize}, 'instrument'), (${U.coin}, 'instrument'),
      (${U.stock}, 'instrument'), (${U.perp}, 'instrument'), (${U.classB}, 'instrument'),
      (${U.xnys}, 'venue'), (${U.listing}, 'listing')`;
    await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${U.usd}, 'US Dollar', ${curated})`;
    await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id, unit_of_measure, contract_multiplier)
      VALUES (${U.wti}, 'commodity', 'WTI crude oil', ${curated}, 'barrel', NULL),
             (${U.maize}, 'commodity', 'Maize', ${curated}, 'metric_ton', NULL),
             (${U.coin}, 'crypto_asset', 'Example Coin', ${curated}, NULL, NULL),
             (${U.stock}, 'equity', 'EXAMPLE CORP', ${curated}, NULL, NULL),
             (${U.perp}, 'perpetual_future', 'kEXC Perpetual', ${curated}, NULL, 1000),
             (${U.classB}, 'equity', 'EXAMPLE CL B', ${curated}, NULL, NULL)`;
    await sql`INSERT INTO venues (id, name, source_record_id)
      VALUES (${U.xnys}, 'New York Stock Exchange', ${curated})`;
    await sql`INSERT INTO listings (id, instrument_id, venue_id, source_id, received_at, source_record_id)
      VALUES (${U.listing}, ${U.classB}, ${U.xnys}, 'undrly-curated', ${at}, ${curated})`;
    await sql`INSERT INTO listing_symbols (listing_id, venue_id, symbol, source_id, received_at, source_record_id)
      VALUES (${U.listing}, ${U.xnys}, 'EXB.B', 'undrly-curated', ${at}, ${curated})`;
    await sql`INSERT INTO identifiers
      (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
      VALUES ('iso4217', 'USD', ${U.usd}, 'currency', 'undrly-curated', ${at}, ${curated}),
             ('mic', 'XNYS', ${U.xnys}, 'venue', 'undrly-curated', ${at}, ${curated})`;
    for (const alias of ["XNYS", "NYSE"]) {
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${U.xnys}, 'venue', ${alias}, 'symbol', 'undrly-curated', ${at}, ${curated})`;
    }
    for (const [node, alias] of [
      [U.wti, "WTI"],
      [U.maize, "MAIZE"],
      [U.coin, "EXC"],
      [U.stock, "EXC"],
      [U.perp, "kEXC-PERP"],
      [U.classB, "EXB.B"],
    ] as const) {
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${node}, 'instrument', ${alias}, 'symbol', 'undrly-curated', ${at}, ${curated})`;
    }
    const feed = async (
      source: string,
      symbol: string,
      subject: string,
      type: string,
      stale: number,
    ) =>
      sql`INSERT INTO quote_feeds (feed_source_id, symbol, subject_id, subject_category, unit_id,
            unit_category, basis, price_type, stale_after_seconds, source_id, received_at, source_record_id)
          VALUES (${source}, ${symbol}, ${subject}, 'instrument', ${U.usd}, 'currency', 'aggregated',
                  ${type}, ${stale}, 'undrly-curated', ${at}, ${curated})`;
    await feed("eia", "RWTC", U.wti, "reference", 1_209_600);
    await feed("worldbank", "MAIZE", U.maize, "average", 5_356_800);
    await feed("kraken", "EXCUSD", U.coin, "last", 300);

    const observe = async (
      source: string,
      subject: string,
      type: string,
      price: string,
      observed: string,
    ) => {
      const rec = await record(source, `${source}-${subject}`, observed);
      const id =
        (
          await sql<{ id: string }[]>`
            INSERT INTO market_observations (subject_id, subject_category, basis, price_type, price,
              unit_id, unit_category, source_id, observed_at, received_at, source_record_id)
            VALUES (${subject}, 'instrument', 'aggregated', ${type}, ${price}::numeric, ${U.usd},
                    'currency', ${source}, ${observed}, ${observed}, ${rec}) RETURNING id::text`
        )[0]?.id ?? "";
      await sql`INSERT INTO canonical_quotes (subject_id, subject_category, unit_id, unit_category,
          method, price, price_type, basis, as_of, eligible_count, computed_at)
        VALUES (${subject}, 'instrument', ${U.usd}, 'currency', 'latest-observation-v1',
                ${price}::numeric, ${type}, 'aggregated', ${observed}, 1, ${observed})`;
      await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
        VALUES (${subject}, ${U.usd}, ${id}, ${price}::numeric)`;
    };
    await observe("eia", U.wti, "reference", "65.12", "2026-09-15T00:00:00Z");
    await observe("worldbank", U.maize, "average", "224", "2026-09-01T00:00:00Z");
    await observe("kraken", U.coin, "last", "1.5", "2026-09-25T12:00:00Z");

    spyRecord = await record("ssga", "https://example.test/spy.xlsx", "2026-09-24T10:00:00Z");
    const snapshot =
      (
        await sql<{ id: string }[]>`
          INSERT INTO universe_snapshots (universe_key, as_of, source_id, received_at, source_record_id)
          VALUES ('sp500', '2026-09-23T00:00:00Z', 'ssga', '2026-09-24T10:00:00Z', ${spyRecord})
          RETURNING id::text`
      )[0]?.id ?? "";
    await sql`INSERT INTO universe_members (snapshot_id, node_id, node_category, rank, source_symbol)
      VALUES (${snapshot}, ${U.stock}, 'instrument', NULL, 'EXC'),
             (${snapshot}, ${U.wti}, 'instrument', NULL, 'AAA')`;

    globalThis.fetch = (async () => {
      fetchCalls++;
      throw new Error("the API must not contact upstream providers");
    }) as unknown as typeof fetch;
  });

  afterAll(async () => {
    globalThis.fetch = realFetch;
    await sql?.end();
    await admin?.unsafe(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`);
    await admin?.end();
  });

  const app = (now: string) => createApp(sql, { staleAfterSeconds: 300, now: () => new Date(now) });

  it("freshness follows each feed's cadence", async () => {
    const tenDays = app("2026-09-25T00:00:00Z");
    const wti = v1.QuoteV1.parse(await (await tenDays.request("/v1/quote/WTI")).json());
    expect(wti.freshness).toBe("fresh"); // 10 d ≤ 14 d; 300 s would say stale
    expect(wti.subject).toMatchObject({ unitOfMeasure: "barrel" });
    expect(wti.subject).not.toHaveProperty("contractMultiplier");
    const later = app("2026-09-30T00:00:01Z"); // 15 d
    expect(v1.QuoteV1.parse(await (await later.request("/v1/quote/WTI")).json()).freshness).toBe(
      "stale",
    );
    const maize = v1.QuoteV1.parse(await (await tenDays.request("/v1/quote/MAIZE")).json());
    expect(maize).toMatchObject({ priceType: "average", freshness: "fresh" });
    const obs = v1.ObservationsV1.parse(
      await (await app("2026-11-03T00:00:00Z").request("/v1/quotes/MAIZE")).json(),
    );
    expect(obs.observations.map((o) => o.freshness)).toStrictEqual(["stale"]); // > 62 d
  });

  it("a pair keeps only the combinations that have a quote feed", async () => {
    const res = await app("2026-09-25T12:00:05Z").request("/v1/resolve?q=EXC/USD");
    const r = v1.ResolveResultV1.parse(await res.json());
    expect(r.status).toBe("resolved");
    expect(r.match).toMatchObject({ kind: "pair", subject: { name: "Example Coin" } });
    // The bare symbol still names two nodes.
    const bare = v1.ResolveResultV1.parse(
      await (await app("2026-09-25T12:00:05Z").request("/v1/resolve?q=EXC")).json(),
    );
    expect(bare.status).toBe("ambiguous");
  });

  it("a venue's common name resolves like its MIC", async () => {
    const resolve = async (q: string) =>
      v1.ResolveResultV1.parse(
        await (
          await app("2026-09-25T00:00:00Z").request(`/v1/resolve?q=${encodeURIComponent(q)}`)
        ).json(),
      );
    const byMic = await resolve("XNYS:EXB.B");
    expect(byMic.status).toBe("resolved");
    expect(byMic.match).toMatchObject({ kind: "node", node: { name: "EXAMPLE CL B" } });
    expect(await resolve("NYSE:EXB.B")).toStrictEqual({ ...byMic, query: "NYSE:EXB.B" });
    expect((await resolve("NYX:EXB.B")).status).toBe("not_found"); // no fuzzy venue names
  });

  it("class-share punctuation: `-` is looked up as `.`, nothing broader", async () => {
    const resolve = async (q: string) =>
      v1.ResolveResultV1.parse(
        await (
          await app("2026-09-25T00:00:00Z").request(`/v1/resolve?q=${encodeURIComponent(q)}`)
        ).json(),
      );
    const dot = await resolve("EXB.B");
    expect(dot.status).toBe("resolved");
    expect(dot.match).toMatchObject({ kind: "node", node: { name: "EXAMPLE CL B" } });
    for (const q of ["EXB-B", "exb-b", "XNYS:EXB-B", "NYSE:EXB-B"]) {
      const r = await resolve(q);
      expect(r.status, q).toBe("resolved");
      expect(r.match, q).toStrictEqual(dot.match);
    }
    for (const q of ["EXBB", "EXB/B", "EXB_B", "EXB-BB", "EXB B"]) {
      expect((await resolve(q)).status, q).toBe("not_found");
    }
  });

  it("/v1/universes lists snapshots; /v1/universes/{key} lists members", async () => {
    const list = v1.UniversesV1.parse(
      await (await app("2026-09-25T00:00:00Z").request("/v1/universes")).json(),
    );
    expect(list.universes).toStrictEqual([
      {
        key: "sp500",
        name: "S&P 500 (via SPY holdings)",
        description:
          "SSGA SPY ETF holdings: a practical proxy, not the official S&P constituent file.",
        source: { id: "ssga" },
        asOf: "2026-09-23T00:00:00Z",
        memberCount: 2,
      },
    ]);
    const sp = v1.UniverseV1.parse(
      await (await app("2026-09-25T00:00:00Z").request("/v1/universes/sp500")).json(),
    );
    expect(sp.sourceRecord).toStrictEqual({ id: spyRecord, key: "https://example.test/spy.xlsx" });
    expect(sp.members.map((m) => [m.sourceSymbol, m.node.name])).toStrictEqual([
      ["AAA", "WTI crude oil"],
      ["EXC", "EXAMPLE CORP"],
    ]);
    for (const [path, status] of [
      ["/v1/universes/nasdaq100", 404],
      ["/v1/universes/unknown", 404],
    ] as const) {
      const res = await app("2026-09-25T00:00:00Z").request(path);
      expect(res.status).toBe(status);
      expect(v1.ErrorV1.parse(await res.json()).error.code).toBe("not_found");
    }
  });

  it("never contacted an upstream provider", () => {
    expect(fetchCalls).toBe(0);
  });
});
