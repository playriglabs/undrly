# Undrly milestones

**Undrly — one normalized API across every market.** Collect fragmented
market data, normalize it, aggregate it, and serve it through one interface.

Status as of 2026-09-26. V1 is tagged `v1.0.0`. V1.1, V1.2 and V1.3 are
committed and verified locally; V1.2 and V1.3 are not yet pushed or tagged.

## At a glance

| Milestone | What it proved | State |
| --- | --- | --- |
| Foundation (phases 0–3) | Canonical identity, graph, provenance, one real source | committed |
| V1: cross-market slice | Five markets, one query language, one quote shape | tag `v1.0.0` |
| V1.1: universes | From 5 instruments to 914, without changing the model | committed |
| V1.2: FX | 37 FX markets, live venue vs. central-bank reference, Southeast Asia | committed |
| V1.3: market data surface | Candles, reference history, market status/statistics, perp data | committed |
| V1.4: cross-ecosystem identity | Chains, deployments, tokenized securities, `/v1/explain` | local, branch `worldsfair` |
| V1.5: Solana production identity | First production chain and deployment: Solana mainnet, Circle's USDC mint | local, branch `worldsfair` |
| V1.6: Robinhood Chain | A tokenized security distinct from its underlying: RHJ's NVIDIA tracker on Robinhood Chain | local, branch `worldsfair` |
| V1.7: Tempo | A payment stablecoin distinct from its fiat: pathUSD on Tempo `TRACKS` USD | local, branch `worldsfair` |
| V1.8: Agent / MCP discovery | Nine read-only MCP tools over the same API code; four ecosystem journeys | local, branch `worldsfair` |
| V1.10: tokenized stocks | xStocks, Backpack Securities and Robinhood tokens tied to their shares by identifier; Nasdaq-100 via QQQ, S&P 400/600; crypto top 500 (Binance USDT); 566 Binance USDⓈ-M perpetuals; Jupiter prices | local, branch `worldsfair` |

**Today:**
- 951 canonical instruments (503 equities, 219 crypto assets, 178
  perpetuals, 37 FX, 14 commodities) and 803 priced markets.
- 22 sources, most for quotes, a few for reference data.
- Eleven read endpoints, and an MCP server (V1.8) with nine read-only tools.
- `scripts/check.sh` is green and `scripts/demo.sh` passes 22/22.

---

## Foundation (phases 0–3)

The data model underneath everything.

- **Canonical identity.** Generated ids (`undrly:<category>:<uuidv7>`)
  for entities, instruments, listings, venues and currencies. They are
  never derived from tickers, ISINs or other external identifiers.
- **External identifiers** with namespace rules: ISIN, FIGI, LEI, MIC,
  ISO 4217, and SEC CIK. Identifier conflicts are quarantined, never
  guessed.
- **Canonical graph.** Typed, directed relationships with provenance
  (`ISSUED_BY`, `TRADES_ON`, `DERIVES_FROM`, `SETTLES_IN`, …).
- **Provenance.** Every source-derived fact traces to the exact raw
  upstream response, stored before anything is derived from it.
- **First real source:** SEC EDGAR (NVIDIA), with a declared User-Agent
  and fair-access pacing.
- **Stack:** a Rust data plane (core, providers, normalization, ingestion,
  storage), a TypeScript read-only API with shared v1 contracts, and
  PostgreSQL with migrations and constraint-level invariants.

## V1: cross-market slice (`v1.0.0`)

One query, one quote shape, five markets.

| Market | Example | Source |
| --- | --- | --- |
| Equity | `NVDA` | Alpaca (IEX) |
| Crypto spot | `BTC/USD` | Kraken + Coinbase, **aggregated** |
| FX | `EUR/USD` | Kraken |
| Commodity | `XAU/USD` | gold-api |
| Perpetual | `BTC-PERP` | Hyperliquid (mark; in USDT since V1.4.1, margined in USDC) |

- **Query language:** symbols, names, pairs (`BTC/USD`), identifiers
  (`isin:…`), venue symbols (`NASDAQ:NVDA`) and canonical ids.
- **Endpoints:** `/v1/quote`, `/v1/quotes`, `/v1/search`, `/v1/resolve`,
  and `/v1/instruments/{id}/graph`.
- **`mean-venue-mid-v1`:** exact-decimal mean of fresh venue mids, with a
  30 s eligibility window and mean bid/ask. It is never presented as a
  best bid/offer.
- **Quote truthfulness:** every quote states `basis`, `priceType`, `unit`,
  `asOf`, `ageMs` (literal) and `freshness` (policy). The API never calls a
  provider while answering.
- **Demo:** `scripts/demo.sh` runs 22 end-to-end checks.

## V1.1: universe expansion

From five instruments to real universes, with the quote architecture
unchanged.

| Universe | Members | Priced by |
| --- | --- | --- |
| `sp500` (via SPY holdings, a proxy) | 503 | Alpaca IEX |
| `crypto-top100` (CoinGecko) | 100 | Kraken / Coinbase via CoinGecko's crosswalk, never by symbol |
| `hyperliquid-perps` | 178 | Hyperliquid, with contract multipliers (`kPEPE` = 1,000) |
| Commodities (curated) | 14 | gold-api, EIA, World Bank (monthly averages) |

- **Deterministic snapshot build.** A local id map, byte-identical
  rebuilds, and an import report listing every exclusion with its reason.
- **No constructed identifiers:** no ISIN is derived from a CUSIP.
- **Per-feed freshness** (EIA 14 days, World Bank 62 days) and units of
  measure (troy ounce, barrel, MMBtu, …).
- **Resolver:** pair disambiguation across asset classes, class-share
  punctuation (`BRK-B` = `BRK.B`), and venue aliases.
- `/v1/universes` and `/v1/universes/{key}`.
- **Data use:** third-party universe data stays local (git-ignored).

## V1.2: FX

- **Model:** FX markets are instruments of class `fx` between two
  currency nodes (base and quote). Orientation is strict: `USD/EUR` is not
  a relabelled `EUR/USD`.
- **Queries:** `EUR/USD`, `eur/usd` and `EURUSD` all resolve to the same
  instrument.
- **Universes:**
  - `fx-major`: 29 pairs.
  - `fx-southeast-asia`: 9 pairs. 10 SEA currencies are modeled.
- **Live vs. reference, never mixed.**
  - Live venue books (Kraken; Kraken + Bitstamp for EUR/USD and GBP/USD)
    are aggregated with `mean-venue-mid-v1`.
  - Central-bank reference rates come from the ECB, the Bank of Canada,
    the Fed H.10, Bank Indonesia (JISDOR), Bank Negara Malaysia and the
    Central Bank of Myanmar. They have no bid/ask and are never averaged
    with live quotes.
- **Explicit inversion.** `1/rate` is computed to the source's precision
  and recorded on each observation.
- **Weekday freshness clock.** A Friday reference rate is not stale at the
  weekend. `ageMs` stays literal.
- **Documented gaps.** USD/PHP, USD/VND and the other unsupported pairs
  each have a written reason. Sources whose terms forbid this use were
  rejected.
- **Determinism:** the FX universe is built from an Undrly-authored spec,
  with pinned ids and byte-identical rebuilds.

## V1.3: market data surface

The same instruments, with more to say about each.

| Endpoint | Returns |
| --- | --- |
| `/v1/candles/{q}?interval=1h\|4h\|1d` | Venue OHLCV. `4h` is derived from four complete hours; gaps are never filled. |
| `/v1/history/{q}` | Reference series (central-bank rates, commodity references) as published, never OHLC |
| `/v1/market/{q}` | Quote plus `marketStatus` and statistics |
| `/v1/derivatives/{q}` | Perpetual mark, oracle (index) price, hourly funding, open interest, 24h volume |
| `/v1/calendar/{q}` | Equity trading days, regular/extended hours, early closes, holidays, corporate actions (dividends, splits, spin-offs, mergers, name changes) and earnings dates with estimates |
| `/v1/economic-calendar` | Scheduled US economic releases (14 curated: CPI, PPI, jobs, claims, JOLTS, GDP, PCE, retail sales, …) |

`marketStatus` values:
- `continuous`: crypto, perpetuals and live FX.
- `open`, `pre_market`, `after_hours`, `closed`: equities, from IEX's
  trading calendar with holidays and early closes.
- `null`: reference rates.

Statistics windows:
- `rolling_24h`: continuous markets.
- `session` with previous close: equities.

Coverage and collection:
- **Candles:** 1h/4h/1d for 503 equities, 81 crypto pairs, 7 live FX
  pairs and 178 perpetuals.
- **Reference history:** 20 FX series and 14 commodity series.
- **Perpetual data:** funding, open interest and oracle price for 178/178
  perpetuals.
- **Collection:** `scripts/history.sh` backfills about 485k bars in about
  25 minutes. Re-runs are idempotent and never duplicate.
- **Latency:** candle queries are index scans; p50 is about 11–15 ms
  across endpoints.
- **Public responses** name no provider or venue in candles and market
  statistics. `/v1/quotes` remains the provenance endpoint.
- **Removed:** `change24h` on `/v1/quote`. A market's change is served by
  `/v1/market` with its window stated.

## V1.4: cross-ecosystem identity

Ontology only: no new provider or production data
([`v1.4-cross-ecosystem-identity.md`](v1.4-cross-ecosystem-identity.md)).

- **Chains** (CAIP-2) and **deployments** (CAIP-19: chain + asset
  namespace + address/mint) as node categories. The same address on two
  chains is two deployments; a Solana mint cannot match an EVM address.
- `deployment REPRESENTS instrument`, `instrument TOKENIZES instrument`
  (wrapped, bridged, share-backed), `DEPLOYED_ON` projected. Class
  `tokenized_security`. No `SAME_AS`.
- `/v1/explain`: the resolver's own result with the rule and stored value
  behind every candidate. Graph gains `deployments` and `markets`.
- Hyperliquid validated on the existing graph (176/178 underlyings).
- **V1.4.1:** Hyperliquid contract semantics from the official
  specification: 176 perpetuals `DENOMINATED_IN` Tether (USDT), PURR and HYPE
  in USD Coin; all `MARGINED_IN` (new) and `SETTLES_IN` USD Coin. Marks are
  now quoted in their denomination. Earlier rows stay as normalized.

## V1.5: Solana production identity

[`v1.5-solana.md`](v1.5-solana.md). No schema change.

- **Solana mainnet** (`solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`) from the
  cluster's own `getGenesisHash`, stored raw; any other cluster is rejected.
- **USDC on Solana**: Circle's published mint, from Circle's own address page
  stored raw, as a deployment that `REPRESENTS` the existing USD Coin.
- `undrly-collect onchain`, driven by the reviewed binding
  `data/reference/onchain.json` (no address in it).
- `scripts/worldsfair.sh`: Hyperliquid and Solana checks over production
  data; Robinhood Chain and Tempo report `NOT_CONFIGURED`.
- Pyth and Jupiter researched, not integrated (no deterministic identity
  mapping; unreviewed terms).
- Perpetual quotes carry bid/ask: each perpetual's Hyperliquid order book
  (`l2Book`) is a second feed; `mark-with-venue-book-v1` keeps the mark as
  price and attaches the book's best levels, with their time (`bidAskAsOf`).

## V1.6: Robinhood Chain / tokenized securities

[`v1.6-robinhood-chain.md`](v1.6-robinhood-chain.md). Research first, then
one production asset.

- Official sources: Robinhood Chain docs, RHJ issuer site, the NVIDIA Stock
  Token's Final Terms, RHJ's asset registry, the chain's own `eth_chainId`.
- Stock Tokens are **collateralised tracker certificates** (debt securities of
  Robinhood Assets (Jersey) Limited) that track the underlying's market value
  with **no legal or beneficial rights** in it: `TRACKS` (new rule, migration
  0021), not `TOKENIZES` or `DERIVES_FROM`.
- Production: Robinhood Chain `eip155:4663`; RHJ (LEI); the NVIDIA Stock Token
  (its own ISIN `JE00BX9C6J83`, `ISSUED_BY` RHJ, `TRACKS` NVIDIA common stock
  by ISIN); its deployment `REPRESENTS` it. Facts from a reviewed,
  SHA-256-pinned transcription of the Final Terms; the address only from the
  registry.
- Bulk import not safe: Undrly's other equities have no authoritative ISIN,
  so their tokens' underlyings cannot be matched (AAPL included).

## V1.7: Tempo / payment identity

[`v1.7-tempo.md`](v1.7-tempo.md). Research first, then one production asset.

- Tempo Mainnet `eip155:4217` from its own `eth_chainId` (Moderato testnet
  42431 rejected). No native gas token; fees are paid in USD TIP-20 tokens.
- pathUSD: TIP-20 predeployed at genesis; the chain's own name, symbol,
  `currency()` and decimals, read in one batch with the chain id and checked
  against the binding (mismatch fails closed). A `crypto_asset` of its own
  that `TRACKS` USD (new rule instrument → currency, migration 0022); never
  USD, USD Coin or Tether.
- Issuer unresolved (Tempo names Bridge; no Bridge source names pathUSD or the
  legal entity): no `ISSUED_BY`. USDC.e (bridged) not ingested.
- No settlement-context endpoint: graph, explain and markets already answer.

## V1.8: Agent / MCP financial discovery

[`v1.8-mcp.md`](v1.8-mcp.md). A thin interface, not a second architecture.

- `typescript/apps/mcp`: MCP server on stdio (official TypeScript SDK v2,
  spec 2026-07-28 and 2025-11-25). It reads through the API's own routes
  (in process over a read-only pool, or over HTTP); no SQL, no resolver of
  its own.
- Nine tools: search, resolve, explain, get_instrument (identifiers + data
  availability per market), graph (bounded, filterable, counted), quote,
  markets (listing / market / feed kept apart), history (bounded candles or
  reference series), derivatives (price unit, margin and settlement kept
  apart). Resource `undrly://vocabulary`. No prompts; no one-shot discovery
  tool (it would have to pick among ambiguous candidates).
- Structured errors: `not_found`, `ambiguous`, `invalid_query`,
  `invalid_identifier`, `no_data`, `unsupported`, `internal` (no leaks).
- API: `?unit=<id>` selects one market (additive).
- Journeys (Hyperliquid, Solana, Robinhood Chain, Tempo) and ten semantic
  invariants frozen in fixture tests; `scripts/mcp.sh` verifies the stdio
  server over production data.

---

## Principles held throughout

- **Truth over uniformity.** `null` or `404 no_data` rather than a guessed
  value. Reference rates are never presented as market quotes; monthly
  averages are never presented as spot.
- **Identity is generated.** No identifier is constructed from
  assumptions. Ids are stable across rebuilds.
- **Raw first.** Every value traces to the stored upstream response and
  its registered source.
- **Exact decimals.** No floating point touches a financial value.
- **Read path never calls upstream.** Collector → normalize → store → API.
- **Sequential, conservative collection.** No scraping, no browser
  impersonation, no terms workarounds.
- **Local/private demo only.** Every source's redistribution rights are
  unreviewed.

## Known limitations

- **Candles need re-running.** `run` refreshes quotes and perp contexts;
  candles and reference history need `scripts/history.sh` again.
- **Coinbase sweep timing.** The sequential Coinbase sweep (about 25 s)
  can leave a two-venue crypto aggregate on one venue.
- **No retention policy** for raw records and observations.
- **Ondo tokens** need an Ondo API key (onboarding); not integrated (V1.10).
- **Weekly FX data.** Some FX pairs (USD/JPY, NZD/USD, USD/SGD, USD/THB)
  rely on the weekly Fed H.10.
- **Unavailable data.** No spot/perp basis (USD vs. USDT units, plus USDC
  pnl; see the V1.4 doc §15 for what it would need), no
  next-funding time, and no oracle-price history.
- **Unreviewed redistribution rights** for every source.

## Proposed next

- Push and tag V1.2 and V1.3.
- Continuous (scheduled) candle refresh, and a retention policy.
- Label `/v1/quotes` internal before any public exposure.
- Phase 5: cross-source identity resolution
  ([`phase-5-identity-resolution.md`](phase-5-identity-resolution.md)).
- Later, per `AGENT.md`: public site and data explorer, SDK, remote
  (Streamable HTTP) MCP, streaming.
