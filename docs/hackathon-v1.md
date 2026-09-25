# Hackathon v1: one normalized API across every market

Status: **approved plan**, implemented in the order below. Deviations found
during implementation are recorded in [§12](#12-deviations-log).

Thesis: *one normalized API across every market.* The differentiator is
broad cross-market coverage behind one consistent model and interface, not
production-grade entity mastering. Phase 5 identity resolution
([`phase-5-identity-resolution.md`](phase-5-identity-resolution.md)) stays a
future architecture document and is **not** part of this scope.

Everything already built is preserved: canonical ids, sources, raw records,
provenance, the identifier layer, graph relationships, normalization,
PostgreSQL persistence, and SEC ingestion.

## 1. Demo universe (exact)

| Market | Representative | Query examples |
| --- | --- | --- |
| Equities | NVIDIA common stock (NVDA) | `NVDA`, `NVIDIA`, `isin:US67066G1040`, `NASDAQ:NVDA` |
| Crypto spot | Bitcoin in USD | `BTC`, `Bitcoin`, `BTC/USD`, `KRAKEN:XXBTZUSD` |
| FX | EUR in USD | `EUR/USD` |
| Commodities | Gold (one troy ounce) in USD | `Gold`, `XAU`, `XAU/USD` |
| Perpetual futures | BTC perpetual on Hyperliquid, in USDC | `BTC-PERP`, `BTC perpetual`, `HYPERLIQUID:BTC` |

No other markets or instruments are in scope.

## 2. Instrument kinds

| Subject | Stored as | Class |
| --- | --- | --- |
| NVIDIA common stock | instrument | `equity` (exists) |
| Bitcoin, USD Coin | instrument | `crypto_asset` (exists) |
| Gold, 1 troy ounce (what ISO 4217 `XAU` denotes) | instrument | **`commodity`** (new) |
| BTC perpetual (Hyperliquid) | instrument | **`perpetual_future`** (new) |
| USD, EUR | currency | (existing category) |

A "pair" is not a node: it is *subject priced in unit*. BTC/USD, EUR/USD
and XAU/USD all use that shape, which is why a price subject may be an
instrument **or a currency** (FX prices EUR).

## 3. Perpetuals

The perpetual is its own instrument, with curated edges:

```text
BTC perpetual ─DERIVES_FROM─▶ Bitcoin
              ─SETTLES_IN───▶ USDC
              ─TRADES_ON────▶ Hyperliquid
```

Its quote is its own **mark price in USDC**. It is not converted to USD for
presentation: USDC is an asset unit, never the USD currency.

## 4. Sources and exact feeds

| Market | Source id | Upstream | Feed symbol | Basis | Price type |
| --- | --- | --- | --- | --- | --- |
| BTC/USD | `kraken` | `GET https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD` | `XXBTZUSD` | venue (Kraken) | last (+ bid/ask) |
| EUR/USD | `kraken` | same request | `ZEURZUSD` | venue (Kraken) | last (+ bid/ask) |
| BTC perpetual | `hyperliquid` | `POST https://api.hyperliquid.xyz/info {"type":"metaAndAssetCtxs"}` | `BTC` | venue (Hyperliquid) | mark |
| XAU/USD | `gold-api` | `GET https://api.gold-api.com/price/XAU` | `XAU` | aggregated | reference |
| NVDA | `alpaca` | `GET https://data.alpaca.markets/v2/stocks/snapshots?symbols=NVDA&feed=iex` | `NVDA` | **venue (IEX)** | last |

The NVDA quote is **an IEX venue quote** delivered by Alpaca
(`basis = venue`, `venue = IEX`, `source = alpaca`): the last trade on IEX,
and only on IEX. It is always presented as an IEX venue quote.

Identity and reference data without an authoritative source in scope
(Bitcoin, USDC, Gold, the perpetual, crypto venues, aliases, feeds) come
from a **curated dataset** (`data/demo/universe.json`, source
`undrly-curated`). It is ingested raw-first like any other source, so every
fact traces to that file.

Every new source starts with `redistribution: unknown` (treated as
restricted). Licensing status per source is documented in
[`sources/`](sources/) and must be reviewed before any use beyond local demo
mode.

## 5. Data flow

```text
providers (Rust, network in undrly-provider `http` only)
  → raw source records (exact response bytes, stored first)
  → normalized market observations (one per feed per response, with basis)
  → aggregation (Rust, core: pure selection)            ← boundary exists even with one feed
  → canonical quote (canonical_quotes: one row per subject × unit, derived cache)
  → API (TypeScript, read-only) / future hot cache
```

- **Observations** keep `basis` (venue / aggregated / derived), price type,
  optional bid/ask, `observed_at` (source time; **null when the source
  states none**), `received_at`, and `source_record_id`.
- **Aggregation v1** selects, per (subject, unit), the latest observation of
  each feed, then the one with the most recent effective time
  (`observed_at`, else `received_at`). With one feed per market this is the
  only eligible observation. No provider ranking, weighting, outlier
  detection, or confidence.
- **Canonical quotes** are written by the collector after each ingestion.
  `GET /v1/quote` reads one indexed row: no upstream call per request.
  Freshness (`fresh` / `stale`) is computed at read time. An in-process or
  Redis cache can sit in front of this table later without changing the
  contract; none is added now.

## 6. Schema changes (migration 0009)

1. `instruments.instrument_class` += `commodity`, `perpetual_future`.
2. `relationship_rules` += `DERIVES_FROM` instrument → instrument.
3. `market_observations` reshaped (it has never held data): `subject_id` +
   `subject_category` (instrument | currency) instead of `instrument_id`;
   `price_type`; nullable `bid`/`ask`; nullable `observed_at`;
   `source_record_id` with the composite provenance FK; replay keys per
   record and per source time.
4. `aliases`: search terms (symbols, names) for any node, with provenance.
   Search only, never identity.
5. `quote_feeds`: *source S's symbol X prices subject Y in unit Z*, at venue V
   or aggregated, with provenance. Drives the collector and
   `VENUE:SYMBOL` resolution.
6. `canonical_quotes`: derived cache, one row per (subject, unit), pointing
   at the selected observation.

## 7. API (TypeScript, Hono, read-only)

| Endpoint | Returns |
| --- | --- |
| `GET /v1/search?q=` | ranked candidates (discovery only) |
| `GET /v1/resolve?q=` | `resolved` / `ambiguous` / `not_found`; never guesses |
| `GET /v1/quote/:query` (also `?q=`) | **one canonical `QuoteV1`** |
| `GET /v1/quotes/:query` (also `?q=`) | the underlying per-feed observations |
| `GET /v1/instruments/:id/graph` | one hop of edges (both directions) + listings |

Resolve accepts: a canonical id; `scheme:value` (`isin`, `figi`, `lei`, `cik`,
`mic`); `VENUE:SYMBOL` (listing symbols and quote feeds); a pair
`BASE/QUOTE`; or an exact alias. Search is case-insensitive over aliases and
names, ranked exact alias > exact name > prefix > substring, and is never
used as identity.

## 8. Collector

`undrly-collect` (Rust binary): `seed` registers sources and ingests the
curated universe; `run` polls sources **sequentially** (one request at a
time, conservative per-source intervals, no retries beyond the next tick),
ingests each response raw-first, then recomputes the canonical quotes it
touched. `--once` does a single pass.

## 9. Build order

1. capture deterministic source fixtures
2. core model changes
3. migration 0009
4. curated reference dataset
5. Kraken adapter and first live quote path
6. Hyperliquid
7. gold-api
8. Alpaca IEX
9. sequential collector
10. TypeScript Hono API
11. demo script

## 10. Out of scope

Phase 5 identity resolution, additional markets, second sources per market,
source ranking / weighting / outliers / confidence, WebSockets and
streaming, historical OHLC, funding history, auth, billing, rate limits,
SDK, MCP, frontend, Redis, markets as nodes, a crypto identifier namespace,
listing-level currency.

## 11. Known limitations

- `search("NVIDIA")` also returns the SEC entity (CIK) as a separate entity
  from the LEI-identified one: that is the honest pre-Phase-5 state.
- Kraken's ticker and Hyperliquid's asset contexts carry no source
  timestamp; their observations have `observed_at = null` and freshness uses
  `received_at`.

## 12. Deviations log

| # | Plan | What was built | Why |
| --- | --- | --- | --- |
| 1 | Kraken feed symbols `XBTUSD`, `EURUSD`; demo query `KRAKEN:XBTUSD` | `XXBTZUSD`, `ZEURZUSD`; `KRAKEN:XXBTZUSD` | Kraken keys its response by pair name, not altname. The request uses the same names, so the payload itself says which feed a price belongs to. `BTC`, `XBT` and `BTC/USD` still resolve through aliases. |
| 2 | Observations carry `observed_at` | `observed_at` is **nullable** | Kraken's ticker and Hyperliquid's contexts state no time. Filling in Undrly's clock would fabricate source time; freshness uses `received_at` instead. |
| 3 | TypeScript API on Drizzle (AGENT.md §6) | `postgres` (postgres.js) with hand-written read-only SQL | Five read queries; an ORM adds nothing yet. Contracts (Zod) validate every response. |
| 4 | API connects with a read-only role (AGENT.md §9) | connections set `default_transaction_read_only`; no separate role yet | Enough for local demo mode; a dedicated role comes with deployment. |
| 5 | `MarketObservationV1` as the API price shape | `QuoteV1` / `ObservationV1`; `MarketObservationV1` marked superseded and not served | FX needs a currency subject; quotes need price type, bid/ask and nullable source time. v1 was never released. |
| 6 | Curated reference data resolves objects by identifiers | Curated objects carry **pinned canonical ids** (UUIDv7 generated once) | Bitcoin, USDC, gold, the perpetual and crypto venues have no identifier in scope. Pinning keeps re-seeding idempotent and demo ids stable across database resets. Ids are generated, never derived. |
| 7 | Alpaca fixture captured in step 1 | Captured later (2026-09-25T07:11Z), once credentials were available; tests used a documented-shape sample until then | Capturing needs an API key. |
| 8 | `SEC EDGAR` in the demo | Not part of `seed`/`run` | Not needed for the five quotes. SEC ingestion is unchanged (`sec_live` test). With it, `search NVIDIA` also shows the separate CIK entity (§11). |
| 9 | — | `GET /v1/quote?q=` / `/v1/quotes?q=` also accepted | Path form with unencoded `/` (`/v1/quote/EUR/USD`) is primary; the query-string form helps clients that encode. |
| 10 | — | `NVIDIA` (entity *and* stock) quotes the stock | Resolve is ambiguous. Quote keeps only candidates that have a canonical quote; the entity has none, so exactly one remains. No ranking is involved. |

## 13. Milestone: multi-source BTC/USD

BTC/USD is the one pair with two independent venue sources. Every other
pair is unchanged (`latest-observation-v1`, one feed).

```text
Kraken   GET /0/public/Ticker?pair=XXBTZUSD,…      → venue observation (last + bid/ask)
Coinbase GET /products/BTC-USD/book?level=1        → venue observation (mid + bid/ask, book time)
                         ↓ (collector, after each ingestion)
          mean-venue-mid-v1  (declared for BTC/USD in data/demo/universe.json)
                         ↓
          canonical_quotes + canonical_quote_inputs  →  GET /v1/quote/BTC/USD
```

### Aggregation method `mean-venue-mid-v1`

1. **Eligible:** for each feed of the pair, its latest observation that is a
   **venue** quote, has **both bid and ask**, and is at most **30 s** old at
   compute time. Age uses the effective time: source time if stated, else
   receipt time.
2. **Venue mid:** `mid_i = (bid_i + ask_i) / 2`, exact, at scale
   `max(scale(bid_i), scale(ask_i)) + 1`.
3. **Canonical price:** `Σ mid_i / n`, at scale `max(scale(mid_i)) + 1`,
   rounded half to even. It is exact for one or two inputs.

The result is labelled `basis = aggregated`, `venue = null`,
`source = null`, `priceType = mid`, with no bid/ask. Its `asOf` is the
**oldest** input's effective time. It is a simple mean of two venues' mids
and nothing more: not a best bid/offer, an execution price, a fair value, an
index, an oracle, or a market-wide price.

No weighting, liquidity or volume scoring, outlier detection, confidence,
source ranking, or fallback heuristics.

### Freshness and fallback (deterministic)

| Fresh eligible observations at compute time | Canonical quote |
| --- | --- |
| 2 | mean of both mids, `eligibleObservations = 2` |
| 1 | that venue's mid, **still `basis = aggregated`**, `eligibleObservations = 1` (inputs name the venue) |
| 0 | **none**: the collector deletes the canonical row, and `/v1/quote` returns 404 `no_quote` |

At read time the API also refuses to serve a `mean-venue-mid-v1` quote whose
`asOf` is older than 30 s (for example, if the collector has stopped): 404
`no_quote`, never stale data labelled fresh. `/v1/quotes` still shows each
venue observation, each marked `fresh` or `stale` against the same 30 s
window.

### Provenance

`canonical_quote_inputs` lists exactly the observations used and the price
each contributed. Every observation names its `source_record_id`, the exact
upstream response bytes. `QuoteV1.aggregation.inputs` exposes
`observationId`, `sourceId`, `venue`, `price` and `sourceRecordId`.
Observations are never copied, changed, or deleted by aggregation. Removing
a canonical quote removes only its input links.

## 14. Scope freeze

Hackathon v1's data architecture is **frozen** as of the approved BTC/USD
milestone:

- **Universe:** NVDA, BTC/USD, EUR/USD, XAU/USD, BTC-PERP. No other markets
  or instruments.
- **Sources:** Kraken, Coinbase, Hyperliquid, gold-api, Alpaca (IEX feed),
  plus curated reference data. No other providers.
- **Aggregation:** `latest-observation-v1` everywhere, and `mean-venue-mid-v1`
  for BTC/USD only. No other methods.
- **Not in scope:** caches, WebSockets, history/OHLC, identity resolution
  (Phase 5), reconciliation, SDK, MCP, auth, billing, frontend, deployment.

Remaining hackathon work is presentation only: README, `GET /`,
`scripts/dev.sh` and `scripts/present.sh`. `scripts/demo.sh` stays the
verification harness. Upstream data remains local/private demo only;
redistribution terms are unreviewed, and no production redistribution
rights are claimed.
