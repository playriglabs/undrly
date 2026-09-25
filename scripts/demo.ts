/**
 * Cross-market demo checks (docs/hackathon-v1.md). Runs the demo queries
 * against a running API, validates every response against the v1 contracts,
 * asserts the market semantics, and prints one line per check with latency.
 *
 *   API_URL=http://127.0.0.1:8787 bun run scripts/demo.ts
 *
 * Exits non-zero if any check fails.
 */
import { v1 } from "../typescript/packages/contracts/src/index.ts";

const API = process.env["API_URL"] ?? "http://127.0.0.1:8787";

type Check = { name: string; ok: boolean; detail: string; ms: number };
const checks: Check[] = [];

async function get(path: string): Promise<{ status: number; body: unknown; ms: number }> {
  const started = performance.now();
  const res = await fetch(`${API}${path}`);
  const body = await res.json();
  return { status: res.status, body, ms: performance.now() - started };
}

async function check(name: string, run: () => Promise<{ detail: string; ms: number }>) {
  try {
    const { detail, ms } = await run();
    checks.push({ name, ok: true, detail, ms });
  } catch (e) {
    checks.push({ name, ok: false, detail: e instanceof Error ? e.message : String(e), ms: 0 });
  }
}

function expect(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function quoteLine(q: v1.QuoteV1): string {
  const where = q.basis === "venue" ? `@ ${q.venue?.name}` : `(${q.basis})`;
  const spread = q.bid === null ? "" : ` [${q.bid} / ${q.ask}]`;
  const unit = q.unit.code ?? q.unit.id;
  const via =
    q.source === null
      ? `${q.aggregation.method} over ${q.aggregation.inputs.map((i) => `${i.venue?.name ?? i.sourceId} ${i.price}`).join(" + ")}`
      : `via ${q.source.id}`;
  return `${q.subject.name}: ${q.price} ${unit} ${q.priceType}${spread} ${where} ${via}, as of ${q.asOf} (${q.freshness})`;
}

type Market = {
  market: string;
  query: string;
  /** Feeds that must each show an observation in /v1/quotes (default 1). */
  minFeeds?: number;
  expect: (q: v1.QuoteV1) => void;
};

const markets: Market[] = [
  {
    market: "equity",
    query: "NVDA",
    expect: (q) => {
      expect(q.subject.kind === "instrument" && q.subject.class === "equity", "NVDA is an equity");
      expect(q.basis === "venue" && q.venue?.name === "IEX", "NVDA is an IEX venue quote");
      expect(q.source.id === "alpaca", "NVDA comes from Alpaca");
      expect(q.unit.kind === "currency" && q.unit.code === "USD", "NVDA in USD");
    },
  },
  {
    market: "crypto spot",
    query: "BTC/USD",
    minFeeds: 2,
    expect: (q) => {
      expect(q.subject.kind === "instrument" && q.subject.class === "crypto_asset", "BTC asset");
      expect(q.unit.kind === "currency" && q.unit.code === "USD", "in USD");
      expect(q.aggregation.method === "mean-venue-mid-v1", "mean of venue mids");
      expect(q.basis === "aggregated" && q.venue === null && q.source === null, "no single venue");
      const venues = q.aggregation.inputs.map((i) => i.venue?.name).sort();
      expect(
        q.aggregation.eligibleObservations === 2 &&
          venues.join() === ["Coinbase Exchange", "Kraken"].join(),
        `both venues contribute (got ${venues.join(", ")})`,
      );
    },
  },
  {
    market: "fx",
    query: "EUR/USD",
    expect: (q) => {
      expect(q.subject.kind === "currency" && q.subject.code === "EUR", "EUR is the subject");
      expect(q.unit.kind === "currency" && q.unit.code === "USD", "in USD");
    },
  },
  {
    market: "commodity",
    query: "XAU/USD",
    expect: (q) => {
      expect(q.subject.kind === "instrument" && q.subject.class === "commodity", "gold");
      expect(q.basis === "aggregated" && q.venue === null, "aggregated, no venue");
      expect(q.priceType === "reference", "reference price");
    },
  },
  {
    market: "perpetual",
    query: "BTC-PERP",
    expect: (q) => {
      expect(
        q.subject.kind === "instrument" && q.subject.class === "perpetual_future",
        "perpetual",
      );
      expect(q.unit.kind === "asset" && q.unit.code === "USDC", "priced in USDC, not USD");
      expect(q.priceType === "mark", "mark price");
      expect(q.basis === "venue" && q.venue?.name === "Hyperliquid", "Hyperliquid venue");
    },
  },
];

for (const m of markets) {
  await check(`quote ${m.market} (${m.query})`, async () => {
    const r = await get(`/v1/quote/${m.query}`);
    expect(r.status === 200, `HTTP ${r.status}: ${JSON.stringify(r.body)}`);
    const q = v1.QuoteV1.parse(r.body);
    m.expect(q);
    return { detail: quoteLine(q), ms: r.ms };
  });
  await check(`quotes ${m.market} (${m.query})`, async () => {
    const r = await get(`/v1/quotes/${m.query}`);
    expect(r.status === 200, `HTTP ${r.status}`);
    const o = v1.ObservationsV1.parse(r.body);
    const min = m.minFeeds ?? 1;
    expect(o.observations.length >= min, `at least ${min} feed observation(s)`);
    const feeds = o.observations
      .map((x) => `${x.venue?.name ?? x.source.id} ${x.priceType} ${x.price} (${x.freshness}, record ${x.sourceRecord.id})`)
      .join("; ");
    return { detail: feeds, ms: r.ms };
  });
}

// The same markets through other query forms.
const sameAs: [string, string][] = [
  ["NVIDIA", "NVDA"],
  ["isin:US67066G1040", "NVDA"],
  ["NASDAQ:NVDA", "NVDA"],
  ["Bitcoin", "BTC/USD"],
  ["KRAKEN:XXBTZUSD", "BTC/USD"],
  ["Gold", "XAU/USD"],
  ["BTC perpetual", "BTC-PERP"],
  ["HYPERLIQUID:BTC", "BTC-PERP"],
];
for (const [alt, canonical] of sameAs) {
  await check(`quote ${alt} = quote ${canonical}`, async () => {
    const [a, b] = [await get(`/v1/quote/${alt}`), await get(`/v1/quote/${canonical}`)];
    expect(a.status === 200 && b.status === 200, `HTTP ${a.status} / ${b.status}`);
    const [qa, qb] = [v1.QuoteV1.parse(a.body), v1.QuoteV1.parse(b.body)];
    expect(qa.subject.id === qb.subject.id && qa.unit.id === qb.unit.id, "same subject and unit");
    return { detail: `${qa.subject.name} in ${qa.unit.code}`, ms: a.ms };
  });
}

await check("resolve BTC (exact alias, not the perpetual)", async () => {
  const r = await get("/v1/resolve?q=BTC");
  const res = v1.ResolveResultV1.parse(r.body);
  expect(res.status === "resolved" && res.match?.kind === "node", "resolved to a node");
  expect(res.match.node.name === "Bitcoin", "Bitcoin");
  return { detail: `${res.method} → ${res.match.node.id}`, ms: r.ms };
});

await check("resolve unknown is not_found", async () => {
  const r = await get("/v1/resolve?q=DOESNOTEXIST");
  const res = v1.ResolveResultV1.parse(r.body);
  expect(res.status === "not_found", "not found");
  return { detail: "not_found", ms: r.ms };
});

await check("search btc", async () => {
  const r = await get("/v1/search?q=btc");
  const s = v1.SearchResultV1.parse(r.body);
  expect(s.results[0]?.node.name === "Bitcoin", "Bitcoin first");
  return { detail: s.results.map((x) => `${x.node.name} (${x.rank})`).join(", "), ms: r.ms };
});

await check("graph of the BTC perpetual", async () => {
  const id = v1.ResolveResultV1.parse((await get("/v1/resolve?q=BTC-PERP")).body);
  expect(id.match?.kind === "node", "perp resolves");
  const r = await get(`/v1/instruments/${id.match.node.id}/graph`);
  const g = v1.GraphV1.parse(r.body);
  const edge = (t: string) => g.edges.find((e) => e.relationshipType === t)?.object.name;
  expect(edge("DERIVES_FROM") === "Bitcoin", "DERIVES_FROM Bitcoin");
  expect(edge("SETTLES_IN") === "USD Coin", "SETTLES_IN USDC");
  expect(edge("TRADES_ON") === "Hyperliquid", "TRADES_ON Hyperliquid");
  return { detail: g.edges.map((e) => `${e.relationshipType} ${e.object.name}`).join(", "), ms: r.ms };
});

let failed = 0;
for (const c of checks) {
  if (!c.ok) failed++;
  const ms = c.ok ? `${c.ms.toFixed(1).padStart(6)} ms` : "        ";
  console.log(`${c.ok ? "PASS" : "FAIL"} ${ms}  ${c.name}: ${c.detail}`);
}
const latencies = checks.filter((c) => c.ok).map((c) => c.ms).sort((a, b) => a - b);
const p50 = latencies[Math.floor(latencies.length / 2)] ?? 0;
console.log(
  `\n${checks.length - failed}/${checks.length} checks passed; API latency p50 ${p50.toFixed(1)} ms, max ${(latencies.at(-1) ?? 0).toFixed(1)} ms`,
);
process.exit(failed === 0 ? 0 : 1);
