# AGENT.md --- Undrly

## 0. Product Identity

**Name:** Undrly\
**Domain:** `undrly.xyz`\
**Category:** Financial data infrastructure / developer infrastructure\
**Tagline:** **Undrly — one normalized API across every market.**

Collect fragmented market data, normalize it, aggregate it, and serve it
through one interface.

Canonical identity, relationships, and provenance are the foundations that
keep that interface correct; they are not the headline.

Financial markets are fragmented across exchanges, chains, vendors,
identifiers, and asset classes. The same economic exposure may appear as
an equity, derivative, ETF holding, index constituent, tokenized asset,
oracle feed, prediction market, or liquidity venue.

Undrly normalizes these fragmented representations into one model and
serves them through one developer API; underneath, it connects them in a
canonical financial instrument graph.

> Markets are fragmented. The underlying isn't.

Undrly is **not** a trading terminal, brokerage, portfolio app, or
generic price API. The core product is infrastructure: one normalized
interface over fragmented markets, built on canonical identity,
relationships, provenance, normalized observations, and cross-market
discovery.

---

## 1. Current Product Scope

### V0 is API-only

There is **no product frontend in the current scope**.

Do not build a Next.js application, graph explorer, dashboard, trading
terminal, wallet UI, portfolio UI, or frontend product.

The public-facing website and data explorer may be built later in the
style of a financial data network: product overview, data coverage,
developers, ecosystem, documentation, and eventually a data explorer.

For now, the product is successful when the API and underlying data
model are excellent.

The primary interface is:

```bash
curl "https://api.undrly.xyz/v1/resolve?q=NVDA"
```

---

## 2. Product Thesis

A ticker is not an identity.

`NVDA`, an ISIN, a FIGI, an exchange listing, an option contract, an ETF
exposure, and a tokenized representation are different identifiers or
financial objects that may connect to the same underlying economic
entity.

Undrly provides:

1.  **Canonical identity** --- stable Undrly IDs for financial objects.
2.  **Instrument resolution** --- resolve symbols, identifiers, names,
    contract addresses, and venue-specific representations.
3.  **Relationship graph** --- connect entities, instruments, listings,
    venues, derivatives, funds, indexes, tokens, chains, oracle feeds,
    and markets.
4.  **Normalized observations** --- heterogeneous providers map into one
    canonical contract.
5.  **Provenance** --- important facts retain source, venue, timestamp,
    freshness, and applicable licensing metadata.
6.  **Cross-market discovery** --- answer not merely "what is NVDA?" but
    "where and how does exposure to this underlying exist?"

The two defining API primitives are **Resolve** and **Graph**. Price
data supports the graph; it is not the thesis by itself.

---

## 3. North Star

Undrly should eventually answer:

> "Give me every instrument and market connected to NVIDIA."

Conceptually:

```text
NVIDIA Corporation
├── Equity
│   └── NASDAQ:NVDA
├── Derivatives
├── Funds
├── Indexes
├── Fixed Income
├── Tokenized Representations
│   ├── Solana
│   └── Arbitrum
├── Oracle / Reference Feeds
└── Other verified markets
```

Never fabricate relationships to make a graph appear complete. Unknown
is preferable to wrong.

---

## 4. Architecture Principle

Undrly uses a hybrid Rust + TypeScript architecture.

> **Rust owns the data plane. TypeScript owns the API/control plane.**

### Rust owns

- external feed ingestion
- WebSocket/feed connections
- decoding and high-throughput validation
- normalization
- timestamping
- source reconciliation
- aggregation
- canonical market observations
- streaming infrastructure
- data-plane health

### TypeScript owns

- REST API
- query orchestration
- resolve / graph / search APIs
- metadata APIs
- developer authentication and API keys later
- rate limits and billing later
- TypeScript SDK later
- MCP/agent tools later

Do not use Rust everywhere merely because Rust is fast. Do not put
latency-sensitive high-volume feed processing into TypeScript merely
because it is easier.

---

## 5. V0 Architecture

```text
External Sources
      ↓
Rust Providers
      ↓
Validation
      ↓
Normalization
      ↓
Canonical Domain
      ↓
PostgreSQL
      ↓
TypeScript API
   ↙    ↓    ↘
Resolve Graph Markets
```

V0 does **not** require Kafka, NATS, Redis Streams, ClickHouse, Neo4j,
Elasticsearch, Kubernetes, a service mesh, multiple microservices, or
multi-region collectors.

Introduce infrastructure only after observed workload justifies it.

---

## 6. Technology Stack

### Rust data plane

Use stable Rust.

Preferred baseline:

```text
tokio
axum
serde
serde_json
reqwest
tokio-tungstenite
rust_decimal
sqlx
tracing
thiserror
```

Rules:

- async I/O for network-bound collectors
- decimal-safe representations for financial values
- never use `f32`/`f64` for exact financial values
- raw onchain amounts are atomic unsigned integers (up to 256 bits) plus
  the token's `decimals`; they are stored separately from normalized
  decimal financial values and never forced into `Decimal`. Conversion to a
  normalized `Decimal` is explicit and fails on overflow; it never rounds
  silently
- avoid unnecessary allocation in hot paths
- do not prematurely micro-optimize without measurement
- structured tracing for ingestion paths

### TypeScript API plane

```text
TypeScript
Bun
Hono
Zod
Drizzle
PostgreSQL
Vitest
Biome
```

Use TypeScript strict mode.

### Database

V0 uses PostgreSQL for entities, instruments, identifiers, listings,
venues, markets, graph relationships, sources, normalized observations,
and relevant current/reference prices.

Do not introduce a graph database merely because the product exposes a
graph.

---

## 7. Repository Structure

```text
undrly/
├── rust/
│   ├── crates/
│   │   ├── undrly-core/
│   │   ├── undrly-store/
│   │   ├── undrly-provider/
│   │   ├── undrly-ingest/
│   │   ├── undrly-normalize/
│   │   └── undrly-reconcile/
│   └── Cargo.toml
├── typescript/
│   ├── apps/
│   │   └── api/
│   └── packages/
│       ├── contracts/
│       └── config/
├── database/
│   └── migrations/
├── docs/
├── tests/
├── AGENT.md
└── README.md
```

Do not create empty packages for hypothetical future features such as
SDK, billing, MCP, or streaming.

Crate boundaries:

- `undrly-core` owns canonical domain types and their invariants. It has
  no I/O and no database dependency; database-specific types (`sqlx`, row
  structs, SQL enums) never appear in it.
- `undrly-store` owns PostgreSQL persistence: migrations (in
  `database/migrations/`), the mapping between core types and SQL, and
  repositories. It depends on `undrly-core`, never the reverse.

---

## 8. Canonical Domain Ownership

Canonical financial semantics originate in the Rust domain layer.

```rust
pub struct InstrumentId(Uuid); // generated UUIDv7, see §10

pub enum ObservationBasis {
    Venue(VenueId), // a venue quote always names its venue
    Aggregated,
    Derived,
}

pub enum PriceUnit {
    Currency(CurrencyId), // fiat currency node
    Asset(InstrumentId),  // e.g. a crypto asset such as a stablecoin
}

pub struct MarketObservation {
    pub instrument_id: InstrumentId,
    pub basis: ObservationBasis,
    pub price: Decimal,
    pub unit: PriceUnit,
    pub observed_at: Timestamp,
    pub received_at: Timestamp,
    pub source_id: SourceId,
}
```

Rust owns the canonical domain and the PostgreSQL schema (migrations). TypeScript
reads PostgreSQL and owns the external API contract. Rust and TypeScript
definitions must not silently drift.

---

## 9. Cross-Language Contracts

There are two contracts, with different owners:

1. **Internal: PostgreSQL.** The relational schema is the persistence
   contract between Rust (ingestion/core, writer) and TypeScript (API,
   reader). Migrations are owned by Rust. The canonical domain is stored
   relationally with database constraints, never as JSON blobs. The
   TypeScript API connects with a read-only role.
2. **External: the versioned JSON API contract** (`schemaVersion`), owned by
   the TypeScript API. It may evolve independently of the relational schema.

Formats shared by both (canonical ID text, decimal strings, timestamps) are
pinned by shared fixtures so both languages render them identically.

Example external representation (illustrative; the exact v1 shape is defined
in `typescript/packages/contracts`):

```json
{
  "schemaVersion": 1,
  "instrumentId": "undrly:instrument:<generated>",
  "basis": "venue",
  "venueId": "undrly:venue:<generated>",
  "price": "183.4200",
  "unit": {
    "id": "undrly:currency:<generated>",
    "kind": "currency",
    "code": "USD"
  },
  "observedAt": "2026-09-24T12:00:00Z",
  "receivedAt": "2026-09-24T12:00:00.012Z",
  "sourceId": "example-source"
}
```

A price unit is identified by its canonical `id`. `code` is a display
convenience, never identity. A crypto unit (`"kind": "asset"`) references the
asset's canonical instrument ID; symbols such as `USDC` are not globally
unique and are never used to identify a unit.

Financial decimals crossing the Rust/TypeScript boundary are serialized
as strings, never silently coerced to JavaScript numbers.

Breaking schema changes require deliberate migration/versioning.

---

## 10. Canonical Identity

Canonical IDs are **generated, stable, and opaque**:

```text
undrly:<category>:<uuidv7 as 26-char lowercase Crockford base32>

undrly:entity:01j8zq6x3kf8e9t0v4b2c7d5ma      (illustrative)
undrly:instrument:01j8zq6x3mb1n7p5r2s9t4w6yc  (illustrative)
```

- `category` is the structural node type (`entity`, `instrument`,
  `listing`, `venue`, `currency`, later `chain`, `index`, `oracle_feed`,
  ...). It never encodes asset class, jurisdiction, venue, or ticker, which
  can change or be corrected.
- The opaque part is a generated UUIDv7, stored in PostgreSQL as `uuid`.
- Never derive canonical identity from company names, tickers, venue
  symbols, ISINs, FIGIs, LEIs, MICs, currency codes, contract addresses, or
  any other mutable or external identifier.
- Canonical IDs are never reused or deleted. Merging duplicates is an
  explicit, recorded operation (later phase).

External identifiers map to canonical IDs through the **identifier layer**:

- global identifiers (ISIN, FIGI, LEI, MIC, ISO 4217, ...) with validity
  periods; one external identifier maps to at most one node at any instant
- venue symbols belong to listings and are scoped to their venue, with
  validity periods (symbols change and get reused)
- contract addresses will be scoped to their chain and identify token
  contracts, not necessarily underlying economic identity
- aliases map explicitly to canonical objects
- identifier history is preserved, not overwritten
- validation and normalization are namespace-specific: ISIN (check digit),
  FIGI (format and check digit), LEI (ISO 17442 mod 97), MIC (ISO 10383
  code), ISO 4217 codes, SEC CIK (10-digit zero-padded, no check digit),
  and venue symbols (case and punctuation preserved, meaningful only with
  their venue) each have their own type and rules;
  there is no generic "checksummed identifier"
- a conflicting mapping (an external identifier already mapped to a
  different node for an overlapping period) is rejected and quarantined
  with its full context for investigation. Nodes are never merged
  automatically, and no source is automatically chosen as truth
- identity resolution is deterministic and uses **primary identifiers**
  per category: entity by LEI or SEC CIK, instrument by ISIN, venue by
  MIC, currency by ISO 4217 code, listing by (instrument, venue). No match
  mints a new canonical id; more than one matched node (including a record
  whose LEI and CIK point to different entities) rejects the record as
  ambiguous. A record's primary identifiers are all assigned to the node it
  resolves to; records that share no primary identifier are never linked
  (by name or otherwise), even when they describe the same company.
  Names and venue symbols never select a node. Secondary identifiers (e.g.
  FIGIs, venue symbols) are claims assigned to the resolved node and are
  quarantined on conflict; they never change which node a record resolves
  to. Adding a primary identifier for a new category is an architectural
  decision

Earlier examples such as `undrly:equity:US:NVDA` are superseded: `NVDA` is a
venue symbol, `US` a jurisdiction, and neither is identity.

---

## 11. Canonical Graph

Initial node types:

```text
Entity
Instrument
Listing
Venue
Market
Asset
Currency
Index
Fund
Derivative
Chain
Token
OracleFeed
DataSource
```

Currency is a first-class node so facts such as `DENOMINATED_IN` and
`SETTLES_IN` can point at it. Fiat currencies (`currency` category) and
crypto assets (instruments of class `crypto_asset`) remain distinct, even
if they later share an `Asset` abstraction. `USDT` is never `USD`. A price
unit is either a currency or an asset.

Initial relationship types:

```text
ISSUED_BY
LISTED_ON
TRADES_ON
DENOMINATED_IN
UNDERLYING_OF
DERIVES_FROM
TRACKS
HOLDS
MEMBER_OF
TOKENIZES
REPRESENTS
PRICED_BY
AVAILABLE_ON
SETTLES_IN
RELATED_TO
```

Prefer precise relationships over `RELATED_TO`.

Every relationship should carry provenance where available.

**Only one canonical direction is stored.** Inverse traversal is derived at
query time, never persisted as a duplicate edge. Canonical direction is
`dependent → thing it depends on`:

```text
instrument  ISSUED_BY       entity
instrument  TRADES_ON       venue
instrument  DENOMINATED_IN  currency | asset
instrument  SETTLES_IN      currency | asset   (what cash flows are paid in)
instrument  MARGINED_IN     currency | asset   (collateral; V1.4.1)
derivative  DERIVES_FROM    underlying      (inverse view: UNDERLYING_OF)
fund        HOLDS           instrument
fund        TRACKS          index | instrument
instrument  MEMBER_OF       index
instrument  TOKENIZES       instrument      (wrapped/bridged/share-backed claim → what backs it)
instrument  TRACKS          instrument      (tracker → what its terms track; no claim on it; V1.6)
instrument  TRACKS          currency        (stablecoin → its reference currency; never that currency; V1.7)
deployment  REPRESENTS      instrument      (the instrument on one chain)
instrument  PRICED_BY       oracle feed
instrument  AVAILABLE_ON    venue | chain   (not stored: a deployment expresses chain presence)
```

- `DEPLOYED_ON` (deployment → chain) is projected from the deployment's
  chain, never stored. The `Token` node of §11 is the `deployment` category
  (V1.4, `docs/v1.4-cross-ecosystem-identity.md`), identified by CAIP-19.

- `UNDERLYING_OF` and `TRACKED_BY` (explain: who TRACKS this node) are
  query-time inverse labels only; they are never stored.
- `LISTED_ON` is projected from the listings table, not stored as an edge.
- `RELATED_TO` requires a symmetric-ordering rule before it may be stored.
- Phase 2 `graph_edges` rows are **current assertions** by a source, not
  eternal historical truth. Edge validity periods are deferred; the schema
  must allow a source to assert the same edge for several validity periods
  later.
- A relationship type becomes storable only when its endpoint node types
  exist; unknown is preferable to wrong.

---

## 12. Provider Boundary

Never allow:

```text
Provider JSON → TypeScript API response
```

Required flow:

```text
Provider payload
→ Provider decoder
→ Validation
→ Normalization
→ Canonical domain object
→ Persistence
→ API
```

Model capabilities separately where useful:

```text
InstrumentProvider
QuoteProvider
SearchProvider
StreamingProvider
ReferenceDataProvider
```

Avoid giant interfaces that force providers to fake unsupported
capabilities.

Ingestion rules:

- providers decode payloads into provider-native records; normalization
  (pure, no I/O) validates them into canonical values; ingestion resolves
  identities and persists
- the raw payload is stored byte-for-byte in `source_records` as evidence
  (bytes, not parsed JSON), deduplicated per source, record key and payload
  hash
- one source record is ingested in **one transaction**: a failure rolls
  back every write of that record; quarantined conflicts are an outcome and
  are committed
- ingestion never modifies existing canonical objects (e.g. a different
  display name from a later source); resolving such differences belongs to
  reconciliation
- repositories (`undrly-store`) are persistence primitives: typed,
  per-table operations with no normalization, reconciliation,
  source-priority, or provider logic. An operation that must be atomic uses
  its own transaction, or a savepoint inside the caller's transaction

Real-source rules:

- **network access lives only in `undrly-provider`**, behind its `http`
  feature. `undrly-core`, `undrly-normalize`, and `undrly-store` never
  perform network access; decoding and normalization stay pure functions
  of bytes
- **raw first:** the exact response body is persisted as a
  `source_record` before any canonical fact is derived from it, in the
  same transaction. Nothing transforms the body before it is stored (no
  content encoding is negotiated, so the stored bytes are the served
  document). The request URL is the record key
- a fetch failure returns before any database access and writes nothing
- provider response structures are provider-native types in
  `undrly-provider`; they never appear in `undrly-core` or storage
- a source asserts only what it is authoritative for. A source's
  incidental fields (e.g. SEC's tickers and exchanges) never become
  listings, venues, symbols, or identifiers of another kind
- requests are conservative: declared `User-Agent` with a contact
  (configured by the operator, never hardcoded), one request per call,
  timeouts and size limits, no concurrency, no automatic retries, no
  redirects, no crawling
- the normal test suite never touches the internet: providers are tested
  against captured responses and local servers; live integration tests
  are `#[ignore]`d and run explicitly

---

## 13. Normalization

Normalization may include:

- symbol mapping
- venue mapping
- currency normalization
- timestamp normalization
- decimal normalization
- identifier mapping
- asset-class mapping
- unit normalization
- source metadata
- freshness classification

Normalization is semantic conversion, not simple field renaming.

---

## 14. Reconciliation

Different providers may disagree. Preserve source observations and make
derived values explicit.

Potential classifications:

```text
raw
venue-specific
aggregated
derived
indicative
delayed
stale
```

Never label an aggregated or derived value as a raw venue quote.

Prices flow through an explicit aggregation boundary, even when only one
source exists:

```text
providers → raw source records → market observations (per source, with basis)
  → aggregation (named, versioned method) → canonical quote → API / cache
```

- observations keep `basis` (venue / aggregated / derived) and never lose
  their source; a venue quote is always presented as that venue's quote
- the API serves canonical quotes from storage (or a cache in front of
  it); it never calls an upstream provider per request
- a source that states no observation time gets none; Undrly's receipt
  time is never presented as source time

---

## 15. Provenance

Provenance is first-class.

Every important observation should answer:

```text
Where did it come from?
Which source produced it?
Which venue does it describe?
When did the source observe it?
When did Undrly receive it?
How fresh is it?
May Undrly redistribute it?
```

Never expose restricted upstream data publicly unless redistribution is
permitted.

---

## 16. Data Acquisition Rules

Do not design Undrly around unauthorized scraping or redistribution.

Respect exchange licensing, provider terms, redistribution restrictions,
attribution requirements, rate limits, delay requirements, and
geographic restrictions.

For MVP/hackathon work, prefer public permitted APIs, open datasets,
delayed data, testnets, developer programs, and explicitly licensed
feeds.

---

## 17. API V0

```http
GET /health
GET /v1/resolve?q=NVDA
GET /v1/search?q=NVIDIA
GET /v1/instruments/{id}
GET /v1/instruments/{id}/graph
GET /v1/instruments/{id}/markets
GET /v1/instruments/{id}/price
GET /v1/sources
```

REST first. No GraphQL unless actual requirements justify it.

API rules:

- explicit API version
- ISO-8601 UTC timestamps
- decimal strings
- cursor pagination where needed
- structured errors
- request IDs
- provenance metadata
- explicit freshness, currency, and units
- provider-neutral canonical responses

---

## 18. Resolve

Resolve is a core primitive.

Potential inputs:

```text
NVDA
NASDAQ:NVDA
NVIDIA
US67066G1040
FIGI
contract address
token symbol
```

Pipeline:

```text
query
→ parse
→ identifier candidates
→ candidate matching
→ canonical entity/instrument
→ confidence / ambiguity
```

Never silently guess an ambiguous instrument.

---

## 19. Graph API

```http
GET /v1/instruments/{id}/graph
```

Conceptual response:

```json
{
  "root": "undrly:instrument:<generated>",
  "nodes": [],
  "edges": [],
  "asOf": "2026-09-24T12:00:00Z"
}
```

Nodes and edges must be independently interpretable. Edges retain
relationship type and provenance. Do not leak arbitrary provider blobs.

---

## 20. Database Model

Phase 2 tables (schema: `database/migrations/`, notes: `docs/persistence.md`):

```text
nodes                canonical ID registry (uuid + category)
entities / instruments / listings / venues / currencies
identifiers          global external identifiers, with validity
listing_symbols      venue-scoped symbols, with validity
identifier_conflicts quarantined conflicting identifier claims
graph_edges          canonical-direction edges, one row per asserting source
relationship_rules   storable types and endpoint categories
sources
market_observations
```

`markets` and a separate `prices` table are deferred until needed. The schema
emerges from the canonical domain.

Storage rules:

- the canonical domain is stored relationally, never as JSON blobs
- canonical IDs are `uuid`; categories are enforced with composite
  foreign keys to `nodes (id, category)`
- normalized financial values use unconstrained `numeric` (preserves
  scale) constrained to `Decimal`'s range
- raw onchain amounts use `numeric(78,0)` with `0 <= value < 2^256`, next
  to the token's `decimals`, never mixed with normalized values
- time ranges use `tstzrange` with exclusion constraints
- conflicting identifier claims go to `identifier_conflicts`, never
  silently overwrite or merge

Index canonical IDs, normalized symbols, external identifiers,
venue+symbol, graph endpoints, and source/timestamp paths as
appropriate.

Use database constraints to protect invariants.

---

## 21. Performance Philosophy

Undrly is intended to become low-latency infrastructure, but V0
prioritizes correctness and clean boundaries before benchmark theater.

1.  Measure before optimizing.
2.  Rust owns latency-sensitive feed processing.
3.  Avoid unnecessary serialization hops.
4.  Use bounded queues when async pipelines are introduced.
5.  Define backpressure explicitly.
6.  Never trade correctness for microseconds without evidence.
7.  Track provider timestamp and Undrly receipt timestamp.
8.  Benchmark hot paths against real workloads.

---

## 22. Streaming Evolution

V0:

```text
Rust ingestion
→ PostgreSQL
→ TypeScript API
```

Later, if realtime consumers justify it:

```text
Rust collectors
→ NATS
  ↙  ↘
stream persistence
         ↓
    PostgreSQL
```

At higher historical volume, evaluate PostgreSQL for
metadata/identity/graph, ClickHouse for high-volume time series, and
Redis/Valkey for hot state/cache.

These are future options, not current requirements.

---

## 23. Failure Handling

Expect:

- provider disconnects
- malformed payloads
- schema changes
- stale feeds
- duplicates
- out-of-order events
- clock differences
- reconnect storms
- rate limits
- database outages

A failed provider must not automatically corrupt canonical state. Use
bounded retry/backoff behavior and avoid tight reconnect loops.

---

## 24. Observability

Rust ingestion paths use structured tracing.

Each provider should expose:

```text
connection state
last message
last valid message
records received/rejected
reconnect count
provider latency
normalization failures
```

Useful quality metrics include unresolved identifiers, ambiguous
mappings, orphan nodes, stale observations, conflicting mappings, and
invalid relationships.

Never log secrets.

---

## 25. Security

- secrets via environment/secret management
- no credentials in repository history
- provider credentials server-side
- strict input validation
- parameterized database access
- rate limiting when public access begins
- secure API-key storage later
- least privilege
- dependency auditing

Financial data integrity is a security property. Malformed upstream data
must not silently mutate canonical identity.

---

## 26. Testing

Rust unit tests cover identifier parsing, decimals, timestamps, provider
decoding, normalization, relationship construction, and reconciliation.

Provider adapters use captured fixtures. Normal tests must not depend on
live APIs.

Database tests (`undrly-store`) run every test against a freshly created
database migrated from empty, and verify database-level invariants by
asserting that invalid rows are rejected by the intended constraint. They
are opt-in locally (they run when `DATABASE_URL` is set and skip otherwise)
and required in CI (`UNDRLY_REQUIRE_DATABASE=1` turns a missing database
into a failure).

TypeScript tests cover routes, validation, response contracts, resolver
orchestration, graph queries, and error mapping.

Critical end-to-end path:

```text
provider fixture
→ Rust decode
→ normalize
→ persist
→ TypeScript API
→ /v1/resolve
→ /v1/instruments/{id}/graph
```

Identical inputs should produce deterministic canonical output.

---

## 27. Initial Vertical Slice

Do not begin with broad coverage.

First reference case:

```text
NVDA
```

Goal:

```text
reference fixture/provider
→ Rust adapter
→ canonical NVIDIA entity
→ canonical NVDA instrument
→ NASDAQ listing/venue
→ verified relationships
→ PostgreSQL
→ TypeScript API
```

Then:

```bash
curl "http://localhost:3000/v1/resolve?q=NVDA"
```

and:

```bash
curl "http://localhost:3000/v1/instruments/<id returned by resolve>/graph"
```

must return valid canonical responses.

Only then add more providers/assets.

---

## 28. MVP Asset Expansion

After the first vertical slice, expand deliberately into candidates such
as US equities, crypto, FX, selected commodities, and selected
onchain/tokenized representations.

Indonesia equities may be added where legal data access exists.

Optimize for correctness of identity and relationships, not vanity
coverage numbers.

---

## 29. Hackathon Strategy

Undrly remains chain-agnostic. Hackathon integrations are adapters/graph
domains, not company identity.

### Solana / Colosseum

A Solana-oriented demo can connect:

```text
canonical instrument
→ tokenized representation
→ Solana token
→ market
→ liquidity venue
→ oracle/reference feed
```

Solana must provide genuine product utility.

### Arbitrum Open House

Potential emphasis:

- RWA discovery
- tokenized representations
- agentic finance
- cross-market identity
- provenance
- financial-agent tooling

Core Undrly semantics remain unchanged between hackathons.

---

## 30. Agent / MCP Layer --- Later

Agents are future consumers, not V0.

Potential tools:

```text
resolve_instrument
get_instrument
get_instrument_graph
get_markets
get_price
find_onchain_representations
```

LLMs must never become the authority for financial equivalence.
Deterministic Undrly data remains authoritative.

---

## 31. SDK --- Later

Eventually:

```ts
const nvda = await undrly.resolve("NVDA");
const graph = await undrly.graph(nvda.id);
```

Keep the SDK thin. Do not duplicate canonical logic in clients. Do not
build the SDK before HTTP contracts stabilize.

---

## 32. Public Website --- Later

Potential future surface:

```text
undrly.xyz
├── overview
├── data
├── network
├── developers
└── ecosystem

docs.undrly.xyz
api.undrly.xyz
status.undrly.xyz
```

A future explorer is for discovery/observability, not a consumer trading
frontend.

No frontend dependencies in V0.

---

## 33. Business Model --- Later

Potential future tiers: Free, Developer, Startup, Enterprise.

Potential monetization: API requests, realtime streams, identifier
resolution, graph queries, historical/bulk datasets, enterprise
licensing, custom datasets, SLA/support.

Do not build billing before users demonstrate what they value.

---

## 34. Moat

Source code is not the moat.

```text
Canonical IDs
→ Identifier history
→ Normalized observations
→ Relationship graph
→ Provenance
→ Resolution quality
→ Historical graph
→ Developer integrations
```

The graph compounds in value as mappings, history, and provenance
improve.

---

## 35. Explicit Non-Goals

Do not build without deliberate scope change:

```text
brokerage execution
portfolio management
social/copy trading
custody
wallet
payments
generic AI chat
proprietary exchange
custom blockchain
full trading terminal
consumer investment app
frontend dashboard
```

Undrly is infrastructure.

---

## 36. Coding-Agent Rules

Before changing code:

1.  Read this entire file.
2.  Identify which architectural layer owns the behavior.
3.  Preserve the Rust data-plane / TypeScript control-plane boundary.
4.  Preserve canonical/provider separation.
5.  Avoid infrastructure not required by the current phase.

While coding:

6.  Prefer readable code over clever abstractions.
7.  Keep modules small.
8.  Make illegal states difficult to represent.
9.  Use decimal-safe financial types.
10. Validate external data.
11. Preserve provenance.
12. Handle ambiguity explicitly.
13. Add tests with behavior changes.
14. Use captured fixtures for provider tests.
15. Never fabricate production data/provider support/relationships.
16. Never commit credentials.
17. Never bypass licensing restrictions.
18. Never silently convert financial decimals to floating point.

Before finishing:

19. Run Rust formatting.
20. Run Rust clippy.
21. Run Rust tests.
22. Run TypeScript type checking.
23. Run TypeScript formatting/linting.
24. Run TypeScript tests.
25. Report changes and known limitations.
26. Never claim completion while tests fail.

---

## 37. Definition of Done

A feature is complete when implementation works, important behavior is
tested, types are sound, formatting/static checks pass, errors are
handled, provenance is retained, financial values remain decimal-safe,
public contract changes are documented, no secrets are exposed, and no
unsupported licensing assumption is introduced.

---

## 38. Build Phases

### Phase 0 --- Foundation

Repository, Rust workspace, TypeScript workspace, PostgreSQL development
setup, format/lint/test commands, README, configuration.

No product features.

### Phase 1 --- Canonical Domain

Implement IDs, Entity, Instrument, Venue, Listing, Source, Relationship,
MarketObservation, timestamps, and decimal rules.

Test invariants thoroughly.

### Phase 2 --- Persistence

Minimal PostgreSQL schema/repositories for the first vertical slice.

### Phase 3 --- First Provider / Fixture Pipeline

One permitted/reference provider integration or deterministic fixture
path:

```text
decode → validate → normalize → persist
```

### Phase 4 --- Resolver

Implement:

```http
GET /v1/resolve?q=NVDA
```

Support exact identifiers first. Do not overbuild fuzzy search.

### Phase 5 --- Graph

Implement:

```http
GET /v1/instruments/{id}/graph
```

Return verified canonical relationships with provenance.

### Phase 6 --- Market Observations

Implement markets and price endpoints while keeping source semantics
explicit.

### Phase 7 --- Additional Domains

Add providers/asset classes one at a time.

### Phase 8 --- Realtime

Only after useful API semantics exist: live collectors, streaming,
backpressure, reconnection, fanout. Evaluate an event bus only when
justified.

### Phase 9 --- Developer Product

Later: API keys, usage, docs, SDK, MCP, public website, data explorer.

---

## 39. First Milestone

A clean checkout should eventually support:

```bash
docker compose up -d

# run Rust pipeline / deterministic reference seed
...

# run TypeScript API
...

curl "http://localhost:3000/v1/resolve?q=NVDA"

curl "http://localhost:3000/v1/instruments/<id returned by resolve>/graph"
```

Only relationships backed by chosen reference data/fixtures may appear.

This proves:

```text
provider
→ Rust
→ canonical model
→ PostgreSQL
→ TypeScript
→ Resolve
→ Graph
```

That is the foundation of Undrly.

---

## 40. Long-Term Vision

Undrly becomes an addressable graph of the financial world.

```text
company
↕
securities
↕
listings
↕
venues
↕
derivatives
↕
fund exposure
↕
indexes
↕
tokenized assets
↕
onchain markets
↕
oracle feeds
↕
liquidity
```

Applications integrate once instead of independently rebuilding
identity, mapping, normalization, and provenance for every market.

> **Undrly — one normalized API across every market.**
