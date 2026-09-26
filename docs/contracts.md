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
receivedAt, asOf, ageMs, freshness, change24h,
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

All decimals (`price`, `bid`, `ask`, `spread`, `spreadBps`, and
`change24h`'s `absolute`, `percent`, `from`) are canonical decimal
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

**`change24h`** is `{ absolute, percent, from, asOf }` or `null`:

- `absolute = price - from`, exact, at the larger scale;
- `percent = (price - from) / from × 100`, half to even at **4** places,
  a decimal string without `%`;
- `from` is the baseline price and `asOf` the baseline's own time.

The baseline is the canonical quote the pair's **own method** would have
served, fresh, at `τ = asOf - 24 h`, recomputed from stored observations
(which are never deleted), from **exactly the same feeds** as the current
quote:

- `latest-observation-v1`: the pair's latest observation at or before `τ`
  must be of the current quote's feed (source, venue, price type) and at
  most the feed's freshness window (300 s for market data) before `τ`;
- `mean-venue-mid-v1`: each feed's latest venue observation with bid and
  ask at or before `τ`, at most 30 s before `τ`; their mean of mids by the
  same rule, if those feeds are exactly the current inputs' feeds (a
  two-venue aggregate is never compared with a single venue, or the other
  way round). Its `asOf` is the oldest baseline input's time.

`change24h` is `null` when no such baseline exists, when the baseline price
is zero, and always for equities (session-traded: no previous close is
presented as a 24-hour change) and for `reference` / `average` prices
(daily to monthly series; gold-api's reference price included). Crypto
spot, perpetual marks and Kraken FX qualify once 24 hours of observations
exist. The baseline's observations are not named in the response; they are
reproducible from `market_observations`.

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

## Changing the API contract

- Adding a key is breaking, because readers reject unknown keys. Any shape
  change is a new version (`src/v2/`, `tests/fixtures/api/v2/`).
- Released fixtures are append-only. v1 has not been released yet.
