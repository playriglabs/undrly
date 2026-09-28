/**
 * MCP verification (docs/v1.8-mcp.md §14), run by `scripts/mcp.sh`: starts
 * the real stdio server (`src/stdio.ts`) as a child process, drives it with
 * the official MCP client in both protocol eras, and checks the four World's
 * Fair journeys over the database in DATABASE_URL. No LLM is involved.
 *
 *   DATABASE_URL=… bun run src/verify.ts
 *
 * Exits non-zero if any check fails.
 */
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { v1 } from "@undrly/contracts";

const SOLANA = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const SOLANA_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const RH = "eip155:4663/erc20:0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";
const RH_ISIN = "JE00BX9C6J83";
const NVDA_ISIN = "US67066G1040";
const TEMPO_PATH_USD = "eip155:4217/erc20:0x20c0000000000000000000000000000000000000";
const TOOLS = [
  "search_instruments",
  "resolve_instrument",
  "explain_instrument",
  "get_instrument",
  "get_instrument_graph",
  "get_quote",
  "get_markets",
  "get_history",
  "get_derivatives",
];

if (process.env["DATABASE_URL"] === undefined && process.env["UNDRLY_API_URL"] === undefined) {
  console.error("verify: set DATABASE_URL (or UNDRLY_API_URL)");
  process.exit(1);
}

const results: { name: string; ok: boolean; detail: string }[] = [];
async function check(name: string, run: () => Promise<string>) {
  try {
    results.push({ name, ok: true, detail: await run() });
  } catch (e) {
    results.push({ name, ok: false, detail: e instanceof Error ? e.message : String(e) });
  }
}
function expect(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function open(mode: "legacy" | "modern") {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [new URL("./stdio.ts", import.meta.url).pathname],
    env: Object.fromEntries(
      Object.entries(process.env).filter((e): e is [string, string] => e[1] !== undefined),
    ),
    stderr: "ignore",
  });
  const client = new Client(
    { name: "undrly-mcp-verify", version: "0" },
    mode === "modern" ? { versionNegotiation: { mode: { pin: "2026-07-28" } } } : {},
  );
  await client.connect(transport);
  return client;
}

const legacy = await open("legacy");
const modern = await open("modern");

type Result = { isError: boolean; data: Record<string, unknown> & { error?: { code: string } } };
async function call(name: string, args: Record<string, unknown>, client = legacy): Promise<Result> {
  const r = await client.callTool({ name, arguments: args });
  return { isError: r.isError === true, data: r.structuredContent as Result["data"] };
}
async function ok(name: string, args: Record<string, unknown>) {
  const r = await call(name, args);
  expect(!r.isError, `${name} ${JSON.stringify(args)}: ${JSON.stringify(r.data)}`);
  return r.data;
}
async function explained(query: string) {
  const e = v1.ExplainV1.parse(await ok("explain_instrument", { query }));
  const c = e.candidates[0];
  expect(e.status === "resolved" && c?.resolution.kind === "node", `${query} resolves to one node`);
  const node = c.resolution.node;
  const rel = (t: string) => c.relationships.filter((r) => r.relationshipType === t);
  return { node, rel, c };
}

await check("initialization: 2025-11-25 initialize handshake", async () => {
  expect(legacy.getProtocolEra() === "legacy", "legacy era");
  expect(legacy.getServerVersion()?.name === "undrly", "server identifies as undrly");
  return `protocol ${legacy.getNegotiatedProtocolVersion()}`;
});

await check("initialization: 2026-07-28 server/discover", async () => {
  const d = await modern.discover();
  expect(d.supportedVersions.includes("2026-07-28"), "offers 2026-07-28");
  expect(
    d.capabilities.tools !== undefined && d.capabilities.resources !== undefined,
    "tools and resources",
  );
  const r = await call("resolve_instrument", { query: "BTC-PERP" }, modern);
  expect(!r.isError && r.data["status"] === "resolved", "a tool call in the modern era");
  return `protocol ${modern.getNegotiatedProtocolVersion()}, versions ${d.supportedVersions.join(",")}`;
});

await check("tool listing and vocabulary resource", async () => {
  const { tools } = await legacy.listTools();
  expect(JSON.stringify(tools.map((t) => t.name)) === JSON.stringify(TOOLS), "nine tools in order");
  expect(
    tools.every((t) => t.annotations?.readOnlyHint === true && t.outputSchema),
    "read-only, typed",
  );
  const res = await legacy.readResource({ uri: "undrly://vocabulary" });
  const vocab = JSON.parse((res.contents[0] as { text: string }).text);
  expect(vocab.invariants.length === 10, "ten invariants");
  return `${tools.length} tools; undrly://vocabulary ${vocab.relationships.stored.length} relationship rules`;
});

let perpId = "";
await check("Hyperliquid: resolve + explain BTC-PERP", async () => {
  const r = await ok("resolve_instrument", { query: "BTC-PERP" });
  const match = (r as v1.ResolveResultV1).match;
  expect(match?.kind === "node" && match.node.class === "perpetual_future", "a perpetual");
  perpId = match.node.id;
  const { rel } = await explained("BTC-PERP");
  const name = (t: string) =>
    rel(t)
      .map((x) => x.object.name)
      .join(",");
  expect(name("DERIVES_FROM") === "Bitcoin", "DERIVES_FROM Bitcoin");
  expect(name("TRADES_ON") === "Hyperliquid", "TRADES_ON Hyperliquid");
  expect(name("DENOMINATED_IN") === "Tether", "DENOMINATED_IN Tether");
  expect(name("MARGINED_IN") === "USD Coin", "MARGINED_IN USD Coin");
  expect(name("SETTLES_IN") === "USD Coin", "SETTLES_IN USD Coin");
  return `${match.node.name}: DERIVES_FROM Bitcoin, DENOMINATED_IN Tether, MARGINED_IN/SETTLES_IN USD Coin`;
});

await check("Hyperliquid: graph", async () => {
  const g = await ok("get_instrument_graph", { id: perpId, direction: "outgoing" });
  const counts = (g["edgeCounts"] as { outgoing: Record<string, number> }).outgoing;
  for (const t of ["DERIVES_FROM", "DENOMINATED_IN", "MARGINED_IN", "SETTLES_IN", "TRADES_ON"]) {
    expect(counts[t] === 1, `one ${t}`);
  }
  return JSON.stringify(counts);
});

await check("Hyperliquid: quote is the mark in USDT", async () => {
  const q = v1.QuoteV1.parse((await ok("get_quote", { query: "BTC-PERP" }))["quote"]);
  expect(q.priceType === "mark", "mark");
  expect(q.unit.kind === "asset" && q.unit.code === "USDT", `unit USDT (got ${q.unit.code})`);
  return `${q.price} ${q.unit.code} (${q.priceType}, ${q.freshness}, as of ${q.asOf})`;
});

await check("Hyperliquid: derivatives context", async () => {
  const r = await ok("get_derivatives", { query: "BTC-PERP" });
  const d = v1.DerivativesV1.parse(r["derivatives"]);
  const contract = r["contract"] as Record<string, { object: { id: string; name: string } }[]>;
  expect(d.unit.code === "USDT", "prices in USDT");
  expect(
    contract["denominatedIn"]?.[0]?.object.id === d.unit.id,
    "DENOMINATED_IN is the price unit",
  );
  expect(contract["marginedIn"]?.[0]?.object.name === "USD Coin", "margined in USD Coin");
  expect(contract["settlesIn"]?.[0]?.object.name === "USD Coin", "settles in USD Coin");
  return `mark ${d.markPrice}, oracle ${d.indexPrice}, funding ${d.fundingRate}/${d.fundingIntervalHours}h, OI ${d.openInterest} (${d.unit.code}; margin/settle USDC)`;
});

await check("Solana: CAIP-19 → deployment → USD Coin on Solana", async () => {
  const { node, rel } = await explained(`caip19:${SOLANA}/token:${SOLANA_MINT}`);
  expect(node.kind === "deployment", "a deployment");
  expect(rel("REPRESENTS")[0]?.object.name === "USD Coin", "REPRESENTS USD Coin");
  expect(rel("DEPLOYED_ON")[0]?.object.name === "Solana", "DEPLOYED_ON Solana");
  const usdc = (await explained("USDC")).node;
  expect(
    usdc.kind === "instrument" && usdc.name === "USD Coin",
    "USDC is USD Coin, not the deployment",
  );
  const bare = await ok("resolve_instrument", { query: SOLANA_MINT });
  expect(bare["status"] === "not_found", "bare mint unresolved");
  return `${node.id} REPRESENTS ${usdc.name} (by ${rel("REPRESENTS")[0]?.provenance.sourceId}); bare mint not_found`;
});

await check("Robinhood Chain: token → RHJ tracker, not NVIDIA stock", async () => {
  const d = await explained(`caip19:${RH}`);
  expect(d.node.kind === "deployment", "a deployment");
  expect(d.rel("DEPLOYED_ON")[0]?.object.name === "Robinhood Chain", "DEPLOYED_ON Robinhood Chain");
  const tracker = await explained(`isin:${RH_ISIN}`);
  expect(d.rel("REPRESENTS")[0]?.object.id === tracker.node.id, "REPRESENTS the tracker");
  expect(tracker.node.class === "tokenized_security", "a tokenized security");
  const share = await explained(`isin:${NVDA_ISIN}`);
  expect(
    share.node.class === "equity" && share.node.id !== tracker.node.id,
    "stock stays distinct",
  );
  expect(tracker.rel("TRACKS")[0]?.object.id === share.node.id, "TRACKS NVIDIA common stock");
  const issuer = tracker.rel("ISSUED_BY")[0]?.object.name ?? "";
  expect(issuer.startsWith("Robinhood Assets (Jersey)"), "ISSUED_BY RHJ");
  expect(
    !share.c.identifiers.some((i) => i.value === RH_ISIN),
    "the stock never gets the token ISIN",
  );
  return `${tracker.node.name} ISSUED_BY ${issuer}, TRACKS ${share.node.name}`;
});

await check("Tempo: pathUSD", async () => {
  const d = await explained(`caip19:${TEMPO_PATH_USD}`);
  expect(d.rel("REPRESENTS")[0]?.object.name === "pathUSD", "REPRESENTS pathUSD");
  expect(d.rel("DEPLOYED_ON")[0]?.object.name === "Tempo", "DEPLOYED_ON Tempo");
  const p = await explained("pathUSD");
  const tracks = p.rel("TRACKS")[0];
  expect(
    tracks?.object.kind === "currency" && tracks.object.name === "US Dollar",
    "TRACKS US Dollar",
  );
  expect(p.rel("ISSUED_BY").length === 0, "no issuer");
  const ids = new Set([p.node.id]);
  for (const q of ["USD", "USDC", "USDT"]) ids.add((await explained(q)).node.id);
  expect(ids.size === 4, "pathUSD, USD, USDC, USDT distinct");
  return `pathUSD TRACKS US Dollar (by ${tracks.provenance.sourceId}); no issuer; four distinct objects`;
});

await check("structured errors", async () => {
  const nf = await call("get_quote", { query: "NO-SUCH-THING-XYZ" });
  expect(nf.isError && nf.data.error?.code === "not_found", "not_found");
  const bad = await call("get_instrument", { id: "undrly:instrument:nope" });
  expect(bad.isError && bad.data.error?.code === "invalid_identifier", "invalid_identifier");
  const big = await call("get_history", { query: "BTC-PERP", limit: 100_000 });
  expect(big.isError && big.data.error?.code === "invalid_query", "bounded history");
  const unsupported = await call("get_markets", { id: (await explained("USD")).node.id });
  expect(unsupported.isError && unsupported.data.error?.code === "unsupported", "unsupported");
  return "not_found, invalid_identifier, invalid_query, unsupported";
});

await legacy.close();
await modern.close();

let failed = 0;
for (const r of results) {
  if (!r.ok) failed++;
  console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}: ${r.detail}`);
}
console.log(`\n${results.length - failed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
