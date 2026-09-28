/**
 * World's Fair verification (docs/v1.5-solana.md §15): end-to-end checks
 * against a running API over production data. Every section verifies real
 * stored identity and provenance; an ecosystem without an authoritative
 * integration reports NOT_CONFIGURED instead of passing.
 *
 *   API_URL=http://127.0.0.1:8787 bun run scripts/worldsfair.ts
 *
 * Exits non-zero if any check fails (NOT_CONFIGURED is not a failure).
 */
import { v1 } from "../typescript/packages/contracts/src/index.ts";

const API = process.env["API_URL"] ?? "http://127.0.0.1:8787";

// Circle's published Solana mainnet USDC mint on Solana mainnet (CAIP-2 from
// the cluster's genesis hash). Expectations only: the data comes from the
// stored Circle and Solana records.
const SOLANA_MAINNET = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const SOLANA_USDC = `${SOLANA_MAINNET}/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`;

type Result = {
  section: string;
  name: string;
  status: "PASS" | "FAIL" | "NOT_CONFIGURED";
  detail: string;
};
const results: Result[] = [];

async function get(path: string): Promise<{ status: number; body: unknown }> {
  const res = await fetch(`${API}${path}`);
  return { status: res.status, body: await res.json() };
}

function expect(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function check(section: string, name: string, run: () => Promise<string>) {
  try {
    results.push({ section, name, status: "PASS", detail: await run() });
  } catch (e) {
    results.push({
      section,
      name,
      status: "FAIL",
      detail: e instanceof Error ? e.message : String(e),
    });
  }
}

function notConfigured(section: string, reason: string) {
  results.push({ section, name: "integration", status: "NOT_CONFIGURED", detail: reason });
}

const explain = async (q: string) =>
  v1.ExplainV1.parse((await get(`/v1/explain?q=${encodeURIComponent(q)}`)).body);
const graph = async (id: string) =>
  v1.GraphV1.parse((await get(`/v1/instruments/${id}/graph`)).body);
const node = (e: v1.ExplainV1) => {
  const c = e.candidates[0]?.resolution;
  expect(e.status === "resolved" && c?.kind === "node", `${e.query} resolves to one node`);
  return c.node;
};

// --- Hyperliquid -------------------------------------------------------------

await check(
  "Hyperliquid",
  "BTC perpetual: derivative of Bitcoin, priced in USDT, margined and settled in USDC",
  async () => {
    const e = await explain("BTC-PERP");
    expect(node(e).class === "perpetual_future", "a perpetual");
    const rel = (t: string) =>
      e.candidates[0]?.relationships
        .filter((r) => r.relationshipType === t)
        .map((r) => r.object.name);
    expect(JSON.stringify(rel("DERIVES_FROM")) === '["Bitcoin"]', "DERIVES_FROM Bitcoin");
    expect(JSON.stringify(rel("DENOMINATED_IN")) === '["Tether"]', "DENOMINATED_IN Tether");
    expect(JSON.stringify(rel("MARGINED_IN")) === '["USD Coin"]', "MARGINED_IN USD Coin");
    expect(JSON.stringify(rel("SETTLES_IN")) === '["USD Coin"]', "SETTLES_IN USD Coin");
    const d = v1.DerivativesV1.parse((await get("/v1/derivatives/BTC-PERP")).body);
    expect(d.unit.code === "USDT" && d.unit.kind === "asset", "derivatives unit USDT");
    return `mark ${d.markPrice} ${d.unit.code}, index ${d.indexPrice}, funding ${d.fundingRate}/h, OI ${d.openInterest}`;
  },
);

await check(
  "Hyperliquid",
  "perpetual quotes: the mark with the venue book's bid/ask and its time",
  async () => {
    const lines = [];
    for (const q of ["BTC-PERP", "ETH-PERP", "kPEPE-PERP"]) {
      const quote = v1.QuoteV1.parse((await get(`/v1/quote/${q}`)).body);
      expect(quote.basis === "venue" && quote.priceType === "mark", `${q} is a venue mark`);
      expect(quote.aggregation.method === "mark-with-venue-book-v1", `${q} method`);
      expect(quote.bid !== null && quote.bidAskAsOf !== undefined, `${q} has its book`);
      const lagMs = Date.parse(quote.asOf) - Date.parse(quote.bidAskAsOf);
      expect(Math.abs(lagMs) <= 60_000, `${q} book within 60 s of the mark`);
      lines.push(
        `${q} ${quote.price} [${quote.bid}/${quote.ask}] book ${Math.round(lagMs / 1000)} s before mark`,
      );
    }
    return lines.join("; ");
  },
);

await check("Hyperliquid", "PURR and HYPE stay USDC-denominated", async () => {
  const units = [];
  for (const q of ["HYPE-PERP", "PURR-PERP"]) {
    const d = v1.DerivativesV1.parse((await get(`/v1/derivatives/${q}`)).body);
    expect(d.unit.code === "USDC", `${q} in USDC`);
    units.push(`${q} ${d.unit.code}`);
  }
  return units.join(", ");
});

await check(
  "Hyperliquid",
  "USDT-priced perpetual candles are served under the corrected unit",
  async () => {
    const r = await get("/v1/candles/BTC-PERP?interval=1h&limit=3");
    expect(r.status === 200, `candles status ${r.status}`);
    const c = v1.CandlesV1.parse(r.body);
    expect(c.unit.code === "USDT", `candles unit ${c.unit.code}`);
    expect(c.candles.length > 0, "has candles");
    return `${c.candles.length} bars in ${c.unit.code}, latest open ${c.candles.at(-1)?.openTime}`;
  },
);

// --- Solana ------------------------------------------------------------------

let deploymentId = "";
await check(
  "Solana",
  "USDC is the USD Coin economic asset (not a deployment, not USD)",
  async () => {
    const e = await explain("USDC");
    const n = node(e);
    expect(n.kind === "instrument" && n.class === "crypto_asset", "a crypto asset instrument");
    expect(n.name === "USD Coin", "USD Coin");
    const usd = node(await explain("USD"));
    const usdt = node(await explain("USDT"));
    expect(
      usd.kind === "currency" && usd.id !== n.id && usdt.id !== n.id,
      "USD, USDT, USDC distinct",
    );
    return `${n.name} (${n.class}) ${n.id}; matched ${e.candidates[0]?.matches.map((m) => `${m.rule}:${m.value}`).join(", ")}`;
  },
);

await check("Solana", "USD Coin → its Solana deployment (graph)", async () => {
  const usdc = node(await explain("USDC"));
  const g = await graph(usdc.id);
  const d = g.deployments?.find((x) => x.caip19 === SOLANA_USDC);
  expect(d !== undefined, `a deployment ${SOLANA_USDC}`);
  expect(d.chain.kind === "chain" && d.chain.name === "Solana", "on the Solana chain");
  const edge = g.edges.find(
    (x) => x.relationshipType === "REPRESENTS" && String(x.subject.id) === String(d.id),
  );
  expect(edge?.object.id === usdc.id, "the deployment REPRESENTS USD Coin");
  expect(edge.provenance.sourceId === "circle", "asserted by Circle's record");
  deploymentId = d.id;
  return `${d.id} ${d.caip19} (REPRESENTS asserted by ${edge.provenance.sourceId} at ${edge.provenance.receivedAt})`;
});

await check("Solana", "the CAIP-19 id resolves to the deployment, not to USD Coin", async () => {
  const e = await explain(`caip19:${SOLANA_USDC}`);
  const n = node(e);
  expect(n.kind === "deployment" && n.id === deploymentId, "the deployment node");
  const c = e.candidates[0];
  expect(
    c?.matches[0]?.rule === "identifier" && c.matches[0].namespace === "caip19",
    "matched by CAIP-19",
  );
  const rel = c.relationships.map(
    (r) =>
      `${r.relationshipType} ${r.object.name} (${r.projected ? "projected" : r.provenance.sourceId})`,
  );
  expect(
    c.relationships.some(
      (r) => r.relationshipType === "REPRESENTS" && r.object.name === "USD Coin",
    ),
    "REPRESENTS USD Coin",
  );
  expect(
    c.relationships.some((r) => r.relationshipType === "DEPLOYED_ON" && r.object.kind === "chain"),
    "DEPLOYED_ON a chain",
  );
  return `${n.kind} ${n.name}: ${rel.join(", ")}`;
});

await check(
  "Solana",
  "Solana mainnet is a chain by its CAIP-2 id; the bare mint is not identity",
  async () => {
    const chain = node(await explain(`caip2:${SOLANA_MAINNET}`));
    expect(chain.kind === "chain" && chain.name === "Solana", "the Solana chain");
    const bare = await explain("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    expect(bare.status === "not_found", "a mint without its chain resolves nothing");
    return `${chain.id} = ${SOLANA_MAINNET}; bare mint → ${bare.status}`;
  },
);

// --- Not yet integrated ------------------------------------------------------

// --- Robinhood Chain (V1.6) ----------------------------------------------------

// RHJ's Final Terms for the NVIDIA Stock Token (docs/v1.6-robinhood-chain.md).
const RH_CHAIN = "eip155:4663";
const RH_TOKEN_ISIN = "JE00BX9C6J83";
const NVDA_ISIN = "US67066G1040";
const RH_DEPLOYMENT = `${RH_CHAIN}/erc20:0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec`;

await check("Robinhood Chain", "the chain is eip155:4663 (its own eth_chainId)", async () => {
  const chain = node(await explain(`caip2:${RH_CHAIN}`));
  expect(chain.kind === "chain" && chain.name === "Robinhood Chain", "Robinhood Chain");
  return `${chain.id} = ${RH_CHAIN}`;
});

await check(
  "Robinhood Chain",
  "the NVIDIA Stock Token is RHJ's tracker, distinct from NVIDIA common stock",
  async () => {
    const e = await explain(`isin:${RH_TOKEN_ISIN}`);
    const token = node(e);
    expect(token.class === "tokenized_security", "a tokenized security");
    const c = e.candidates[0];
    const rel = (t: string) => c?.relationships.filter((r) => r.relationshipType === t) ?? [];
    const issuer = rel("ISSUED_BY");
    expect(
      issuer.length === 1 && issuer[0]?.object.name === "Robinhood Assets (Jersey) Limited",
      "issued by RHJ, not NVIDIA",
    );
    const share = node(await explain(`isin:${NVDA_ISIN}`));
    expect(share.class === "equity" && share.id !== token.id, "the share is its own equity");
    const tracks = rel("TRACKS");
    expect(tracks.length === 1 && tracks[0]?.object.id === share.id, "TRACKS NVIDIA common stock");
    expect(rel("TOKENIZES").length === 0 && rel("DERIVES_FROM").length === 0, "only TRACKS");
    expect(
      c?.relationships.every((r) => r.provenance.sourceId === "rhj-final-terms") === true,
      "asserted by the Final Terms record",
    );
    // Identifiers stay on their own object.
    expect(
      JSON.stringify(c?.identifiers) ===
        JSON.stringify([{ namespace: "isin", value: RH_TOKEN_ISIN }]),
      "the token has only its own ISIN",
    );
    const shareExplain = await explain(`isin:${NVDA_ISIN}`);
    const shareIds = shareExplain.candidates[0]?.identifiers.map((i) => i.value) ?? [];
    expect(!shareIds.includes(RH_TOKEN_ISIN), "the share never gets the token's ISIN");
    const shareIssuer = shareExplain.candidates[0]?.relationships.find(
      (r) => r.relationshipType === "ISSUED_BY",
    );
    expect(shareIssuer?.object.name === "NVIDIA Corporation", "the share keeps its issuer");
    return `${token.name} (${token.class}) TRACKS ${share.name}; issuer ${issuer[0]?.object.name}`;
  },
);

await check(
  "Robinhood Chain",
  "its deployment REPRESENTS the token and is DEPLOYED_ON Robinhood Chain",
  async () => {
    const e = await explain(`caip19:${RH_DEPLOYMENT}`);
    const d = node(e);
    expect(d.kind === "deployment", "a deployment");
    const rels = e.candidates[0]?.relationships ?? [];
    const represents = rels.find((r) => r.relationshipType === "REPRESENTS");
    expect(represents?.object.kind === "instrument", "REPRESENTS an instrument");
    expect(represents.provenance.sourceId === "rhj-api", "asserted by RHJ's registry");
    const token = node(await explain(`isin:${RH_TOKEN_ISIN}`));
    expect(represents.object.id === token.id, "REPRESENTS the token, not the share");
    const on = rels.find((r) => r.relationshipType === "DEPLOYED_ON");
    expect(on?.object.name === "Robinhood Chain" && on.projected, "DEPLOYED_ON Robinhood Chain");
    const bare = await explain("0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC");
    expect(bare.status === "not_found", "an address without its chain is not identity");
    return `${d.id} ${RH_DEPLOYMENT}`;
  },
);

await check(
  "Robinhood Chain",
  "NVDA and AAPL still resolve to the traditional equities",
  async () => {
    const lines = [];
    for (const q of ["NVDA", "AAPL"]) {
      const n = node(await explain(q));
      expect(n.class === "equity", `${q} is an equity`);
      lines.push(`${q} → ${n.name}`);
    }
    return lines.join(", ");
  },
);
// --- Tempo (V1.7) ---------------------------------------------------------------

// Tempo Mainnet and pathUSD, a TIP-20 predeployed at genesis
// (docs/v1.7-tempo.md). Expectations only; the data is the chain's answers.
const TEMPO = "eip155:4217";
const PATH_USD = `${TEMPO}/erc20:0x20c0000000000000000000000000000000000000`;

await check(
  "Tempo",
  "the chain is eip155:4217 (its own eth_chainId), not the testnet",
  async () => {
    const chain = node(await explain(`caip2:${TEMPO}`));
    expect(chain.kind === "chain" && chain.name === "Tempo", "Tempo");
    const testnet = await explain("caip2:eip155:42431");
    expect(testnet.status === "not_found", "Moderato (42431) was never ingested");
    return `${chain.id} = ${TEMPO}`;
  },
);

await check("Tempo", "pathUSD is its own crypto asset that TRACKS USD", async () => {
  const e = await explain("pathUSD");
  const asset = node(e);
  expect(asset.kind === "instrument" && asset.class === "crypto_asset", "a crypto asset");
  const rels = e.candidates[0]?.relationships ?? [];
  expect(rels.length === 1, "one relationship: its reference currency");
  const tracks = rels[0];
  expect(
    tracks?.relationshipType === "TRACKS" && tracks.object.kind === "currency",
    "TRACKS a currency",
  );
  expect(tracks.object.name === "US Dollar", "the US Dollar");
  expect(tracks.provenance.sourceId === "tempo-rpc", "asserted by the chain's own answer");
  expect(!rels.some((r) => r.relationshipType === "ISSUED_BY"), "issuer unresolved, not guessed");
  // USD, USDC, USDT and pathUSD are four distinct objects.
  const ids = new Set([asset.id]);
  for (const q of ["USD", "USDC", "USDT"]) ids.add(node(await explain(q)).id);
  expect(ids.size === 4, "USD, USDC, USDT, pathUSD distinct");
  return `${asset.name} (${asset.class}) TRACKS ${tracks.object.name}`;
});

await check(
  "Tempo",
  "the pathUSD deployment REPRESENTS pathUSD and is DEPLOYED_ON Tempo",
  async () => {
    const e = await explain(`caip19:${PATH_USD}`);
    const d = node(e);
    expect(d.kind === "deployment", "a deployment");
    const rels = e.candidates[0]?.relationships ?? [];
    const represents = rels.find((r) => r.relationshipType === "REPRESENTS");
    expect(represents?.object.name === "pathUSD", "REPRESENTS pathUSD");
    expect(represents.provenance.sourceId === "tempo-rpc", "provenance: the chain");
    const usdc = node(await explain("USDC"));
    expect(represents.object.id !== usdc.id, "not USD Coin");
    const on = rels.find((r) => r.relationshipType === "DEPLOYED_ON");
    expect(on?.object.name === "Tempo" && on.projected, "DEPLOYED_ON Tempo");
    const bare = await explain("0x20c0000000000000000000000000000000000000");
    expect(bare.status === "not_found", "an address without its chain is not identity");
    return `${d.id} ${PATH_USD}`;
  },
);

await check("Tempo", "explain is deterministic and FX stays separate", async () => {
  const a = await explain("pathUSD");
  const b = await explain("pathUSD");
  expect(JSON.stringify(a) === JSON.stringify(b), "same explanation twice");
  const fx = (await get("/v1/resolve?q=EUR/USD")).body as v1.ResolveResultV1;
  expect(
    fx.match?.kind === "pair" && fx.match.unit.code === "USD",
    "EUR/USD priced in the USD currency",
  );
  return "EUR/USD → USD currency; pathUSD TRACKS USD; no USD ↔ stablecoin identity";
});

let failed = 0;
for (const r of results) {
  if (r.status === "FAIL") failed++;
  console.log(`${r.status.padEnd(14)} [${r.section}] ${r.name}: ${r.detail}`);
}
const passed = results.filter((r) => r.status === "PASS").length;
const pending = results.filter((r) => r.status === "NOT_CONFIGURED").length;
console.log(`\n${passed} passed, ${failed} failed, ${pending} not configured`);
process.exit(failed === 0 ? 0 : 1);
