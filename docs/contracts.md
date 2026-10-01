# Contracts

Two contracts, with different owners (`AGENT.md` §9):

| Contract | Owner | Location |
| --- | --- | --- |
| Internal: PostgreSQL schema (Rust writes, TypeScript reads) | Rust (`undrly-store`) | `database/migrations/`, see `docs/persistence.md` |
| External: versioned JSON API | TypeScript | `typescript/packages/contracts/src/v1/` |

The JSON contract is not a serialization of the relational schema and may
evolve independently. Rust does not produce JSON.

## Shared formats

Values both languages render must be identical. They are pinned by
`tests/fixtures/shared/`, which Rust and TypeScript both test:

- canonical ids: `undrly:<category>:<UUIDv7 as 26-char lowercase Crockford base32>`,
  with uuid ↔ text pairs (the API stores UUIDs from PostgreSQL and formats them)
- decimals: canonical `rust_decimal` text, never JSON numbers
- timestamps: canonical RFC 3339 UTC (`Z`, 0/3/6 fractional digits); kept as
  strings in TypeScript because `Date` cannot hold microseconds
- source ids; vocabulary (categories, relationship types and rules,
  observation bases). The database is checked against the same vocabulary.

## API v1 rules

- `"schemaVersion": 1`; any other value is rejected.
- Fixed key order; all keys present (absent values are `null`); unknown keys
  rejected.
- Branded TypeScript types, so an `InstrumentId` cannot be passed as a `VenueId`.
- A price `unit` is `{ "id", "kind", "code" }`. `id` is the canonical
  identity and is authoritative. `kind` is `currency` (fiat, id category
  `currency`, `code` required ISO 4217) or `asset` (crypto asset, id category
  `instrument`, `code` an optional display symbol). A code or symbol such as
  `USDC` is never identity.
- Relationships use the stored vocabulary in canonical direction. Inverse
  labels (`UNDERLYING_OF`) and projections (`LISTED_ON`) are for the Graph
  API to derive, not part of this document type.

Fixtures: `tests/fixtures/api/v1/` (valid documents must round-trip unchanged,
`invalid/` must be rejected).

### Documents served by the API (`typescript/apps/api`)

| Endpoint | Document |
| --- | --- |
| `GET /v1/quote/:query` | `QuoteV1`: the one canonical quote of a subject in a unit |
| `GET /v1/quotes/:query` | `ObservationsV1`: the latest observation of each feed behind it |
| `GET /v1/resolve?q=` | `ResolveResultV1`: `resolved` / `ambiguous` / `not_found` |
| `GET /v1/search?q=` | `SearchResultV1` (discovery only) |
| `GET /v1/instruments/:id/graph` | `GraphV1` |
| `GET /v1/explain?q=` | `ExplainV1` (V1.4): why `resolve` concluded what it did |
| `GET /v1/universes` | `UniversesV1`: each universe with a snapshot (key, name, description, source, `asOf`, `memberCount`) |
| `GET /v1/universes/:key` | `UniverseV1`: the latest snapshot's members (`node`, `rank`, `sourceSymbol`) and its upstream `sourceRecord`; unknown key or no snapshot → 404 |
| errors | `ErrorV1` (`bad_request` 400, `not_found`/`no_quote` 404, `ambiguous` 409) |

`ObservationV1` (`/v1/quotes`) is one source's stored observation:
`subject` (an instrument with its class, or a currency with its code),
`unit`, `priceType`, `price`, `bid`/`ask` (both or neither), `basis`,
`venue` (exactly when `basis` is `venue`, else `null`), `observedAt`
(`null` when the source states no time), `receivedAt`, `observationId`,
`source`, `sourceRecord` (`id`, `key`) and read-time `freshness`.
`MarketObservationV1` is superseded and is not served.

### `QuoteV1` (`/v1/quote`)

One canonical quote, discriminated by `basis`:

```text
schemaVersion, subject, unit,
priceType, price, bid, ask, spread, spreadBps,
basis,
  venue, observedAt                  ← basis = "venue" only
receivedAt, asOf, ageMs, freshness,
aggregation { method, eligibleObservations, computedAt }
```

- **`VenueQuoteV1`** (`basis = venue`): one venue's market. `venue` is
  always set; `observedAt` is the source's time, `null` when the source
  states none. Its method is `latest-observation-v1`.
- **`AggregatedQuoteV1`** (`basis = aggregated`) and **`DerivedQuoteV1`**
  (`basis = derived`): the keys `venue` and `observedAt` are **absent**
  (not `null`): the basis already says no single venue is behind the price.
  `asOf` carries the time.

No canonical quote has a `source` key: the data provider that delivered a
price (e.g. Alpaca for IEX) is not part of the public quote. It is stored
with every observation and served per feed by `/v1/quotes`.

`aggregation` is exactly `{ method, eligibleObservations, computedAt }`. It
does not list the inputs: which observations, venues, sources and raw
records produced a quote is stored (`canonical_quote_inputs` →
`market_observations` → `source_records`) for audit and reproduction, not
served by `/v1/quote`.

For `mean-venue-mid-v1`, `price` is the mean of the eligible venue mids,
`bid` the mean of the same inputs' bids and `ask` the mean of their asks
(same scale; `bid <= price <= ask`; never `null`). These are
mean-of-eligible-venue bid/ask, not a best bid/offer, NBBO or a
consolidated book; with one input they are that venue's own bid and ask.
`/v1/quotes` observations keep their own bid/ask.

All decimals (`price`, `bid`, `ask`, `spread`, `spreadBps`) are canonical decimal
**strings**, never JSON numbers, computed with exact decimal arithmetic
(no floating point). Scale is kept as stored: `price`/`bid`/`ask` carry
the scale the source reported or the aggregation rule produced (e.g.
Kraken's fixed five places, `84145.90000`; a mean of mids at
`max(scale(mid)) + 1`, `84076.2025000`). No normalization is applied.

**Spread.** With both bid and ask:

- `spread = ask - bid`, exact, at scale `max(scale(bid), scale(ask))`;
- `spreadBps = (ask - bid) / price × 10 000`, rounded half to even at
  **4** decimal places (from the exact rational value, one rounding);
  `null` if `price <= 0`.

Without a bid and ask (reference and average series, marks), both are
`null`; no spread is ever estimated. For `priceType = mid`,
`bid <= price <= ask` is required; a `last` or `mark` may lie outside its
bid/ask.

**Age and freshness** are separate:

- `ageMs = max(0, floor(responseTime - asOf))` in milliseconds, computed
  when the response is made (never stored). It uses `asOf`, not
  `receivedAt`: how old the market information is, not when Undrly fetched
  it.
- `freshness` is the policy verdict against the feed's cadence (30 s for
  `mean-venue-mid-v1`, 300 s for market data, 14 days for EIA, 62 days for
  World Bank). A closed-market equity can be hours old and `stale`; a World
  Bank average can be weeks old and `fresh`.

**No 24-hour change on quotes.** An earlier `change24h` field (baseline:
the quote the same method served 24 h before, from the same feeds) was
removed in V1.3: it stayed `null` until Undrly had collected 24 h of quotes.
A market's change is served by `/v1/market` `statistics` instead, with its
window (`rolling_24h` or `session`) stated.

V1.1 additions (v1 is unreleased, so v1 itself was extended):

- `priceType` gains `average`: a published average over a period, e.g.
  World Bank monthly averages.
- An instrument `subject` may carry `contractMultiplier` (e.g. `"1000"` for
  `kPEPE`) and `unitOfMeasure` (`troy_ounce`, `barrel`, `mmbtu`,
  `metric_ton`, `kilogram`). They are omitted, never `null`, when unset, so
  V1 documents are unchanged.
- `freshness` uses the cadence the observation's feed declares
  (`stale_after_seconds`: 300 for market data, 14 days for EIA, 62 days for
  World Bank). `mean-venue-mid-v1` keeps its 30 s window.

V1.3 additions (market data, [`v1.3-market-data.md`](v1.3-market-data.md)):

- `CandlesV1` (`/v1/candles`), `HistoryV1` (`/v1/history`), `MarketV1`
  (`/v1/market`), `MarketsV1` (`/v1/markets`), `DerivativesV1`
  (`/v1/derivatives`); semantics in the schemas' doc comments
  (`typescript/packages/contracts/src/v1/market-data.ts`).
- `MarketsV1` (`/v1/markets?class=&q=&limit=&offset=`): every market with a
  canonical quote, paged (limit 1–100, default 50), ordered by subject name.
  `class` takes a comma-separated list of instrument classes; `q` matches
  subject names and aliases (case-insensitive substring, wildcards literal).
  Each row is `/v1/market`'s body (`null` when no quote is servable now) plus
  a `sparkline` of up to 24 hourly closes. `counts` per subject class ignore
  both filters; `total` is the filtered count. Built for the dashboard's
  market table (docs/v1.8-mcp.md §17).
- Error code `no_data` (404): the market exists, the endpoint has no data
  for it.

V1.2 additions (FX, [`v1.2-fx.md`](v1.2-fx.md)):

- Instrument class `fx`: an FX market (`EUR/USD`) between two currency
  nodes. Its `subject` carries `baseCurrency` and `quoteCurrency`
  (`{ id, code }`), present only for `fx` and required there; the quote
  currency equals `unit`. The price is units of the quote currency per one
  unit of the base currency. `EUR/USD` is served with this subject (V1
  served the EUR currency as the subject).
- A feed may count its freshness window on a `weekdays` clock (Saturdays and
  Sundays, UTC, do not count), for reference rates published on business
  days. `ageMs` stays literal elapsed time.
- Universe keys gain `fx-major` and `fx-southeast-asia`.

V1.4 additions (cross-ecosystem identity,
[`v1.4-cross-ecosystem-identity.md`](v1.4-cross-ecosystem-identity.md)):

- Categories `chain` and `deployment`; instrument class `tokenized_security`.
- Relationship rules `REPRESENTS` (deployment → instrument) and `TOKENIZES`
  (instrument → instrument). `DEPLOYED_ON` is a projection (deployment →
  chain), like `LISTED_ON`, and is never stored.
- Resolve accepts `caip2:<chain id>` and `caip19:<asset type>` (method
  `identifier`). ERC-20 addresses are compared in lowercase.
- `GraphV1` gains `deployments` (`{ id, chain, caip19 }`) and `markets`
  (`{ unit, venues }`), each **omitted** when empty, so earlier documents are
  unchanged.
- V1.6: relationship rule `TRACKS` (instrument → instrument: a tracker whose
  terms track the object's market value, without a claim on it).
- V1.7: relationship rule `TRACKS` instrument → currency (a payment
  stablecoin → the reference currency it is designed to be worth, e.g.
  pathUSD → USD). The token is never the currency; no response shape changes.
- V1.7: explain relationship label `TRACKED_BY`, a projection (`projected:
  true`) listing each instrument that TRACKS the node, with that edge's
  provenance. `explain/NVDA` shows the NVIDIA Stock Token; `explain/USD`
  shows pathUSD. Resolution is unchanged; nothing new is stored.
- V1.5: aggregation method `mark-with-venue-book-v1` (perpetuals): a venue
  mark with the same venue's best bid/ask when its book is within 60 s;
  `VenueQuoteV1.bidAskAsOf` (the book's time) is present exactly then, and
  omitted otherwise, so earlier documents are unchanged.
- V1.4.1: relationship rule `MARGINED_IN` (instrument → currency |
  instrument: the collateral). `DerivativesV1.unit` is the contract's price
  denomination, not the settlement asset; `/v1/derivatives` serves only
  contexts normalized under the market's current unit. Hyperliquid
  perpetuals are quoted in Tether (USDT) except PURR and HYPE (USD Coin).
- `ExplainV1` (`GET /v1/explain?q=`): the resolver's own status, method and
  candidates, each with its `matches` (rule, side, namespace, value, venue,
  source), current `identifiers`, outgoing `relationships` (with the
  projections, marked `projected`) and, for pairs, `quoted`. No scores.

### Derived FX (V1.9)

- `cross-via-stablecoin-v1` (`AggregationMethod`): a `DerivedQuoteV1`
  (`basis: derived`, `priceType: mid`), the ratio of two pairs' canonical
  quotes through a stablecoin, rounded to the fewer significant digits of the
  two (`v1.crossRate`). Its inputs are the legs' observations
  (`canonical_quote_legs`); `asOf` is the older leg.
- `MarketStatisticsV1.window` adds `rolling_24h_closes` (a cross: max/min
  of 25 hourly cross closes, `volume: null`), and for published series
  (reference rates, averages; `marketStatus: null`)
  `rolling_24h_observations` (polled within the day) and
  `previous_publication` (latest vs the value before, `previousClose` set).
  A published series' `/v1/markets` sparkline is its hourly last values or
  its last 24 publications.
- `HistoryV1` adds `priceType: mid` with `basis: derived` for a cross's
  closes (`/v1/history/{q}?interval=1h|1d`); `?series=reference` returns a
  cross pair's own reference feed.
- `MarketQueryV1` (`/v1/markets/query?id=&unit=`): the shortest readable
  query that the resolver maps to exactly that market (`readable: true`),
  else the id form. Readable queries are for people and examples; store ids.
- Universe key `fx-global`. Details: [`docs/v1.9-live-fx.md`](v1.9-live-fx.md).

## Changing the API contract

- Adding a key is breaking, because readers reject unknown keys. Any shape
  change is a new version (`src/v2/`, `tests/fixtures/api/v2/`).
- Released fixtures are append-only. v1 has not been released yet.
