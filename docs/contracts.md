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
| errors | `ErrorV1` (`bad_request` 400, `not_found`/`no_quote` 404, `ambiguous` 409) |

`QuoteV1` and `ObservationV1` share their price fields: `subject` (an
instrument with its class, or a currency with its code), `unit`,
`priceType`, `price`, `bid`/`ask` (both or neither), `basis`, `venue`
(exactly when `basis` is `venue`), `observedAt` (`null` when the source
states no time), `receivedAt` and `source`. `QuoteV1` adds `asOf`,
`freshness` (computed at read time) and `aggregation` (method, number of
eligible observations, computed at, and `inputs`: exactly the observations
used, with each one's contributed price and raw-record id). Its `source` is
`null` for a multi-source aggregate. `ObservationV1` adds `observationId`,
`sourceRecord` (`id`, `key`) and read-time `freshness`. `MarketObservationV1` is superseded and
is not served.

## Changing the API contract

- Adding a key is breaking, because readers reject unknown keys. Any shape
  change is a new version (`src/v2/`, `tests/fixtures/api/v2/`).
- Released fixtures are append-only. v1 has not been released yet.
