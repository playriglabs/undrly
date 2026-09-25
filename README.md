# Undrly

**One graph for every market.**

Undrly is financial data infrastructure: a canonical identity, relationship,
and normalized-observation layer for financial instruments, exposed through
developer APIs. A ticker is not an identity; Undrly's job is to connect the
identifiers, listings, and representations that refer to the same economic
exposure, with provenance for every fact.

`AGENT.md` is the architectural source of truth.

## What Undrly is not

Not a trading terminal, brokerage, portfolio app, wallet, or generic price
API. There is no frontend in the current scope.

## Current status

**Phases 0–2, the Phase 3 NVDA identity slice, the first real source (SEC
EDGAR, NVIDIA only), and the hackathon v1 cross-market slice
([`docs/hackathon-v1.md`](docs/hackathon-v1.md)).**

One read-only API (`search`, `resolve`, `quote`, `quotes`, `graph`) serves
five markets through one contract: NVDA (IEX venue quote via Alpaca),
BTC/USD (Kraken), EUR/USD (Kraken), XAU/USD (gold-api, aggregated) and the
BTC perpetual (Hyperliquid, mark price in USDC). Every quote traces to the
exact upstream response ([`docs/sources/quotes.md`](docs/sources/quotes.md)).

A deterministic fixture record for NVIDIA / NVDA flows through decode,
normalize, identity resolution, and PostgreSQL, and is read back. NVIDIA's
SEC EDGAR submissions document can be fetched live and ingested as an
entity with its CIK, traceable to the exact stored response
([`docs/sources/sec-edgar.md`](docs/sources/sec-edgar.md)). There is no HTTP
API and no market data. Nothing here is production-ready.

## Architecture

```text
External sources → Rust providers → validation → normalization
  → canonical domain → PostgreSQL → TypeScript API (Resolve, Graph, Markets)
```

The canonical domain, the PostgreSQL schema, and the API contract package
exist today.

| Rust — data plane                                    | TypeScript — API/control plane           |
| ---------------------------------------------------- | ---------------------------------------- |
| provider ingestion, decoding, validation             | REST API, query orchestration            |
| normalization, reconciliation, timestamping          | Resolve, Graph, Search, metadata APIs    |
| **canonical financial domain** (origin of semantics) | consumes versioned contracts from Rust   |
| market observations, latency-sensitive processing    | auth, rate limits, SDK, MCP (later)      |

Rules enforced in code:

- Canonical ids are generated UUIDv7s (`undrly:<category>:<id>`), never derived
  from names, tickers, or external identifiers. External identifiers map to
  them through an identifier layer with history.
- Exact financial values use `rust_decimal::Decimal`; `f32`/`f64` are banned by
  clippy. Decimals cross language boundaries as strings (`"183.4200"`).
- Timestamps are UTC with microsecond precision; `observed_at` (source time) and
  `received_at` (Undrly time) are always separate.
- Relationships are stored in one canonical direction and cannot be
  constructed without provenance.
- PostgreSQL is the internal Rust → TypeScript contract; the JSON API contract
  is owned by TypeScript.

Design notes: [`docs/domain.md`](docs/domain.md),
[`docs/persistence.md`](docs/persistence.md),
[`docs/contracts.md`](docs/contracts.md).

## Repository structure

```text
undrly/
├── rust/                       Cargo workspace (data plane)
│   └── crates/
│       ├── undrly-core/        canonical domain (no I/O)
│       ├── undrly-store/       PostgreSQL migrations, mapping, repositories
│       ├── undrly-provider/    provider capabilities, fixture + SEC EDGAR providers (only network code)
│       ├── undrly-normalize/   provider records → validated canonical values (pure)
│       ├── undrly-ingest/      resolve identities + persist, quotes + aggregation
│       └── undrly-collect/     demo collector binary: seed, sequential polling
├── typescript/                 Bun workspace (API plane)
│   ├── apps/
│   │   └── api/                read-only Hono API (search, resolve, quote, quotes, graph)
│   └── packages/
│       └── contracts/          external JSON API contract (Zod)
├── database/migrations/        PostgreSQL schema (owned by undrly-store)
├── tests/fixtures/             shared, identifier, and API fixtures
├── .github/workflows/ci.yml    CI (database tests required)
├── docs/                       design decisions
├── scripts/check.sh            runs every required check
├── compose.yaml                local PostgreSQL 17
└── AGENT.md                    architecture source of truth
```

Not created yet, on purpose (`AGENT.md` §7 lists them as the target layout):

- `undrly-reconcile` — nothing reconciles yet; conflicts are quarantined, not
  resolved.
- `typescript/apps/api` — the HTTP API starts in Phase 4; an empty app would be
  placeholder architecture.
- `typescript/packages/config` — shared config is one `tsconfig.json` and one
  `biome.json` at `typescript/`; a package adds nothing yet.

## Local development

Requirements: stable Rust (via `rustup`; `rust/rust-toolchain.toml` selects
it), Bun ≥ 1.4, Docker (for PostgreSQL 17 and the database tests).

```bash
# everything (what must pass before merging)
./scripts/check.sh

# Rust
cd rust
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

# TypeScript
cd typescript
bun install
bun run typecheck     # tsc, strict
bun run lint          # biome check
bun run format        # biome check --write
bun test              # Bun runner (tests use the Vitest API)
bun run test          # Vitest

# Local PostgreSQL + database tests (opt-in locally, required in CI)
cp .env.example .env  # then set UNDRLY_POSTGRES_PASSWORD
docker compose up -d
DATABASE_URL=postgres://undrly:<password>@127.0.0.1:5432/undrly \
  cargo test -p undrly-store   # each test creates, migrates, drops its own DB

# Cross-market demo: seed, collect once, start the API, run the checks
# (network; add APCA_API_KEY_ID / APCA_API_SECRET_KEY for the NVDA quote)
DATABASE_URL=postgres://undrly:<password>@127.0.0.1:5432/undrly ./scripts/demo.sh
#   or, separately:
#   rust/target/debug/undrly-collect seed && rust/target/debug/undrly-collect run
#   bun typescript/apps/api/src/server.ts        # http://127.0.0.1:8787

# Optional live SEC EDGAR check (network; not part of check.sh)
UNDRLY_SEC_USER_AGENT="Your Name you@example.com" \
DATABASE_URL=postgres://undrly:<password>@127.0.0.1:5432/undrly \
  cargo test -p undrly-ingest --test sec_live -- --ignored --nocapture
```

## Implemented

- Canonical ids: generated UUIDv7 `CanonicalId` with typed `EntityId`,
  `InstrumentId`, `ListingId`, `VenueId`, `CurrencyId`.
- Identifier layer: namespace-specific `Isin`, `Figi`, `Lei`, `Mic`,
  `CurrencyCode`, `VenueSymbol`; `IdentifierAssignment`, `ListingSymbol`,
  `Validity`.
- Domain: `Entity`, `Instrument`, `Venue`, `Currency`, `Listing`, `Source`,
  `Relationship` (canonical direction, provenance), `MarketObservation`
  (`ObservationBasis`, `PriceUnit`).
- PostgreSQL schema (5 migrations, 14 tables) with database-enforced
  invariants, and database tests that migrate a fresh database per test.
- API contract v1 (`MarketObservationV1` with a canonical `unit` object,
  `RelationshipV1`) in TypeScript.
- Repositories in `undrly-store` (typed persistence primitives, atomic
  identifier assignment with conflict quarantine).
- NVDA vertical slice: fixture provider → normalizer → `ingest_reference`
  (identity resolution by primary identifier, one transaction per record).
- Provider boundary: `QuoteProvider` and `ReferenceDataProvider`;
  implementations are the deterministic fixture provider and SEC EDGAR
  (company submissions → entity + CIK, via `ingest_entity`).

## Not implemented

Markets beyond the five demo instruments, second sources per market, source
ranking/weighting/outlier handling, Phase 5 identity resolution
([`docs/phase-5-identity-resolution.md`](docs/phase-5-identity-resolution.md),
future architecture only), streaming/WebSockets, historical OHLC,
authentication, rate limiting, billing, SDK, MCP, and any frontend. Source
data is licensed for **local demo mode only** until each source's terms are
reviewed ([`docs/sources/quotes.md`](docs/sources/quotes.md)). The only
reference data in the repository is the curated demo universe
(`data/demo/universe.json`); other fixture values are illustrative test data.
