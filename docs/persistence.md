# Persistence (Phase 2)

PostgreSQL is the internal contract between the Rust data plane (writer) and
the TypeScript API (reader). The schema lives in `database/migrations/` and is
owned by `rust/crates/undrly-store`, which embeds it (`MIGRATOR`), maps core
types to SQL, and provides repositories. `undrly-core` has no database
dependency.

Requires PostgreSQL ≥ 17 (`uuid_extract_version`) and the `btree_gist`
extension (created by migration 0001).

## Tables

| Migration | Table | Holds |
| --- | --- | --- |
| 0001 | `sources` | data sources; `redistribution` defaults to `unknown` (treated as restricted) |
| 0002 | `nodes` | every canonical id (`uuid` v7) and its immutable category |
| 0002 | `entities`, `instruments`, `venues`, `currencies`, `listings` | one table per category; names are display data |
| 0003 | `identifier_schemes` | which global namespaces may identify which categories |
| 0003 | `identifiers` | ISIN / FIGI / LEI / MIC / ISO 4217 → node, with validity periods |
| 0003 | `listing_symbols` | venue-scoped symbols → listing, with validity periods |
| 0003 | `identifier_conflicts` | quarantined conflicting identifier claims |
| 0004 | `relationship_rules` | storable relationship types and endpoint categories |
| 0004 | `graph_edges` | canonical-direction edges, one row per asserting source |
| 0005 | `market_observations` | normalized prices with basis, unit, provenance, two timestamps |
| 0006 | `source_records` | raw payloads (bytes) as received, deduplicated by source, record key, SHA-256 |
| 0006 | — | `identifier_conflicts_replay_key`: replaying a conflicting claim adds no duplicate |

Shared domains: `display_name`, `financial_decimal` (numeric within
`rust_decimal` range, scale preserved), `validity` (half-open non-empty
`tstzrange`), `source_id`.

No JSON/JSONB columns exist; a test enforces this.

## Invariants enforced by the database

| Invariant | Constraint |
| --- | --- |
| Canonical ids are RFC 9562 UUIDv7 | `nodes_id_check` (`IS NOT DISTINCT FROM 7`: nil and non-RFC variants make `uuid_extract_version` NULL, which a plain `= 7` CHECK would accept) |
| A category row matches its node's category; categories cannot change once referenced | composite FKs `(id, category) → nodes` |
| Referenced nodes cannot be deleted | FKs (`NO ACTION`) |
| An external identifier maps to one node at any instant | `identifiers_one_node_at_a_time` (exclusion) |
| A namespace identifies only allowed categories | FK `(scheme, node_category) → identifier_schemes` |
| Identifier value shape per namespace | `identifiers_value_shape` (check digits are validated in Rust) |
| A venue symbol maps to one listing per venue at a time; reusable after its period | `listing_symbols_one_listing_at_a_time` |
| A listing has one symbol at a time | `listing_symbols_one_symbol_at_a_time` |
| A symbol's venue is its listing's venue | FK `(listing_id, venue_id) → listings (id, venue_id)` |
| Quarantine rows describe a coherent claim against the same identifier | `identifier_conflicts_target`, FKs to `(id, scheme, value)` / `(id, venue_id, symbol)` |
| Only canonical directions and enabled types are stored | FK `(relationship_type, subject_category, object_category) → relationship_rules` |
| Edge endpoints have their declared categories | composite FKs to `nodes` |
| No self-edges | `graph_edges_no_self_edge` |
| One assertion per edge, source, and period | `graph_edges_one_assertion_per_period` (exclusion) |
| Venue basis ⇔ venue present | `market_observations_basis_venue` |
| Price unit is a currency or asset node, not the instrument itself | `unit_category` CHECK, composite FK, `market_observations_not_self_denominated` |
| Prices fit `Decimal` exactly; no NaN/Infinity | `financial_decimal_check` |
| Observation replays are idempotent (NULL venues included) | `market_observations_replay_key` (`UNIQUE NULLS NOT DISTINCT`) |
| Every edge, listing, identifier, observation has provenance | `source_id NOT NULL REFERENCES sources`, `received_at NOT NULL` |

Rust-only checks (the database cannot see them): ISIN/FIGI/LEI check digits,
that an asset unit is a `crypto_asset`, and that a node row is created in its
category table in the same transaction.

## Decisions made during implementation

- **Edge validity is deferred without blocking it.** `graph_edges` has a
  `valid_during` column, and uniqueness is an exclusion constraint over
  `(subject, type, object, source, valid_during &&)`, not a UNIQUE on
  `(subject, type, object, source)`. `graph_edges_validity_deferred` pins every
  row to an unbounded period for now, so rows are current assertions. To
  introduce edge validity later, drop that one CHECK; a test proves that
  disjoint periods are then accepted and overlapping ones are rejected.
- **Decimals cross the boundary as text.** sqlx 0.9's `rust_decimal` codec
  drops the scale of zero in both directions (`0.00` becomes `0`). Non-zero
  values are unaffected. `undrly_store::mapping::{decimal_to_sql,
  decimal_from_sql}` bind canonical text with `$n::numeric` and read with
  `::text`. A test pins the sqlx behaviour so a future fix is noticed.
- **Identifier conflicts are quarantined, not resolved.** The exclusion
  constraint rejects the claim. The writer (future repository) records it in
  `identifier_conflicts` with the claimed node, period, source, receipt time,
  and a reference to the existing mapping it collided with. Nothing is merged
  and no source is chosen as truth. Claims for nodes that do not exist yet,
  and raw provider payloads, are out of scope until Phase 3.
- **`LISTED_ON` comes from `listings`**, and `UNDERLYING_OF` is the query-time
  inverse of `DERIVES_FROM`; neither can be stored as an edge.
- **Relationship rules seeded now:** `ISSUED_BY`, `TRADES_ON`,
  `DENOMINATED_IN`, `SETTLES_IN`. Other types get rules when their node
  categories or classes (fund, index, derivative, token, chain, oracle feed)
  exist.

## Repositories

Persistence primitives only: typed operations per table, taking
`&mut PgConnection`. No normalization, reconciliation, source-priority, or
provider logic. No generic CRUD abstraction.

| Module | Operations |
| --- | --- |
| `sources` | `insert_source`, `get_source`, `insert_source_record`, `get_source_record` |
| `reference` | `get_node`, `has_object`, `insert_entity`/`get_entity`, `insert_instrument`/`get_instrument`, `insert_venue`/`get_venue`, `insert_currency`/`get_currency`, `insert_listing`/`get_listing`, `listings_for_instrument` |
| `identifiers` | `assign_identifier` → `AssignOutcome`, `resolve_identifier(at)`, `identifier_history`, `identifiers_for_node` |
| `listing_symbols` | `assign_listing_symbol` → `SymbolAssignOutcome`, `resolve_listing_symbol(venue, at)`, `listings_with_symbol(at)`, `symbols_for_listing` |
| `graph` | `insert_relationship`, `relationships_from` (forward), `relationships_to` (inverse) |
| `conflicts` | `record_identifier_conflict`, `conflicts_for_identifier`, `conflicts_for_listing_symbol` |

Semantics:

- **Inserts are idempotent and never overwrite.** Identical content returns
  `Write::Unchanged`; different content for an existing key is
  `StoreError::ExistingRecordDiffers`.
- **There is no public "insert node".** A node is created only together with
  its object row, so storage never holds a node without its object.
- **Assignments return explicit outcomes.** `Assigned`, `Unchanged`, or
  `Conflict` (the claim was quarantined and the existing mapping is
  returned). Two other outcomes write nothing: `OverlapsExistingPeriod`
  (same node, different overlapping period) and, for symbols,
  `ListingHasOtherSymbol`. None of them overwrites a mapping.
- **Replays keep the first provenance** for single-row facts (identifier
  assignments, listing symbols, listings). Graph edges are per source, so a
  second source's assertion is its own row.

### Transaction boundaries

| Operation | Atomic unit |
| --- | --- |
| `insert_entity`/`instrument`/`venue`/`currency`/`listing` | node row + object row |
| `assign_identifier`, `assign_listing_symbol` | overlap check (rows locked `FOR UPDATE`) + insert, or overlap check + quarantine rows. A race lost to a concurrent writer is caught by the exclusion constraint inside a savepoint and handled as a conflict. |
| `undrly_ingest::ingest_reference` | the whole record: raw record, nodes, identifiers, symbol, edges, quarantine rows |

Each repository operation opens its own transaction. Called inside a
caller's transaction, it becomes a savepoint, so ingestion composes
repositories into one per-record transaction.

## Schema limitations found in Phase 3

- **A listing claiming a different symbol can't be quarantined.**
  (`ListingHasOtherSymbol`: the listing already has another symbol for the
  period.) `identifier_conflicts` references a collision on the same symbol
  value. The outcome is reported to the caller but not persisted.
- **Corroboration is not recorded.** A second source asserting the same
  identifier, symbol or listing leaves the single existing row, with the
  first source's provenance.
- **Facts are not linked to their raw record.** Provenance is `source_id` +
  `received_at`. There is no `source_record_id` on facts, so which record
  asserted a fact is inferred, not stored.
- **No listing-level trading currency.** The slice asserts `DENOMINATED_IN`
  on the instrument. A listing's quote currency (listing → currency) has no
  rule yet.
- **No category for FIGI composite level.** The US composite FIGI is
  neither the global security nor a single listing, so it is not stored.
- **Listings have no natural key in the schema.** Several listings of one
  instrument on one venue are allowed. Ingestion treats that as ambiguous
  rather than choosing one.
- **No claims for nodes that don't exist.** Conflicts reference an existing
  claimed node. A claim about a node that was never created can't be
  quarantined.
- **No conflict detection for edges.** Contradictory edges (e.g. two issuers
  for one instrument) are both stored, per source.

## Deferred

A read-only role for the API (created when the API exists), `updated_at`
tracking, node merges/redirects, `markets`, a latest-price table, onchain
amounts (`numeric(78,0)` + `decimals`, `AGENT.md` §20), partitioning, and
observation repositories (market-price ingestion).

## Testing

The shared harness is `undrly_store::testing` (feature `testing`). It is
used by `rust/crates/undrly-store/tests/` and `rust/crates/undrly-ingest/tests/`. Every test creates a fresh database,
applies all migrations from empty, and drops it afterwards. Rejection tests
assert both the SQLSTATE and the name of the constraint that fired.

- Locally: set `DATABASE_URL` (role needs `CREATEDB`). Without it, the tests
  skip (and report as passed; `scripts/check.sh` prints a notice).
- CI: `UNDRLY_REQUIRE_DATABASE=1` makes a missing database a failure
  (`.github/workflows/ci.yml` runs PostgreSQL 17 as a service).

Drift checks against the database: `relationship_rules` equals
`RelationshipType::allowed_endpoints` and the shared fixture;
`identifier_schemes` equals `Namespace::categories`; category/kind/class CHECKs
equal core names; every Rust-valid identifier passes the database shape check.
