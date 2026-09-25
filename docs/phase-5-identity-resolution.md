# Phase 5 proposal: cross-source identity resolution

Status: **proposal for review**. No code or migrations exist for anything in
this document.

## 1. Problem

The real SEC slice produced a correct but incomplete result:

| Source | Identifier | Node |
| --- | --- | --- |
| reference fixture (GLEIF view) | LEI `549300S4KLFTLO7GSQ80` | entity **A** (`NVIDIA CORPORATION`) |
| SEC EDGAR | CIK `0001045810` | entity **B** (`NVIDIA CORP`) |

SEC reports `"lei": null`, so no record links the two identifiers. Two
canonical entities for one company is the right answer given the evidence:
names, tickers and exchange names are not identity.

The system needs a way to record, later, that A and B are one real-world
entity. That must happen without deleting or reusing either id, without
rewriting any source assertion, and without an irreversible merge.

Today a record carrying both identifiers is rejected as `Ambiguous` and
writes nothing, not even its raw record (tested in
`sec_and_lei_only_records_do_not_link_without_a_shared_identifier`). The
most valuable evidence we could receive is currently thrown away.

## 2. Recommended architecture (summary)

Three layers, each with its own table family:

```text
 source assertions        evidence                  decisions                 derived
 (what sources said)      (what sources said        (what Undrly concluded)   (read path)
                           about sameness)
 identifiers        ──▶   coreference_claims   ──▶  identity_links      ──▶  identity_canonical
 graph_edges, listings    + claim members           (active | retracted)      (node → canonical)
 all → source_records     → source_records          → claims / operator record
```

1. **Evidence (co-reference claims).** A source record that describes one
   object with several primary identifiers asserts that those identifiers
   identify the same object. The claim is stored as a set of *external
   identifiers*, not Undrly nodes, with `source_record_id` provenance.
   Evidence is never interpreted, merged, or deleted.
2. **Decisions (identity links).** A link says that node X and node Y (same
   category) are the same real-world object. Each link records who decided
   it (a named, versioned deterministic policy, or an operator backed by a
   source record) and which claims justify it. A link can only be
   retracted; it is never edited or deleted.
3. **Clusters and the canonical id.** The active links form connected
   components (clusters). The canonical id of a cluster is its
   **earliest-minted member**, i.e. the smallest UUIDv7. No id is minted and
   none is retired. Every original id stays valid and resolvable forever. A
   derived cache (`identity_canonical`) maps each clustered node to its
   canonical id. It is maintained in the same transaction as links and can
   be rebuilt at any time.
4. **Read-time union.** Identifiers, edges, listings and provenance stay on
   the node that received them. A canonical view unions the facts of all
   cluster members at query time and tags each fact with the member it is
   attached to. Nothing is copied or reparented.
5. **Policy is separate.** Evidence storage and link storage are
   persistence primitives in `undrly-store`. Deciding to link belongs to a
   new `undrly-reconcile` crate. Its first policy, `coreference-v1`, is
   deterministic: it links only when a claim is internally consistent and
   the result would not break a cluster invariant. Anything else is
   surfaced as a conflict. There is no ranking, scoring, or fuzzy matching.

## 3. Research: how mature systems handle this

Concepts from reference-data and entity-mastering practice, and what we
take from each:

| System / practice | Mechanism | Take / reject |
| --- | --- | --- |
| **GLEIF LEI registration status** ([LEI-CDF 3.1](https://www.gleif.org/en/lei-data/access-and-use-lei-data/level-1-data-lei-cdf-3-1-format)) | A duplicate registration is marked `DUPLICATE`, the non-surviving record, with a `SuccessorEntity`/`SuccessorLEI` pointer. `MERGED`/`RETIRED`/`DUPLICATE`/`ANNULLED` records are retained "for query resolution and historical reasons". | **Take:** survivor + pointer, never delete, old ids keep resolving. **Note:** GLEIF's duplicate case means one real entity *can* legitimately carry two LEIs. Our uniqueness rules must allow an explicit operator override for this. |
| **GLEIF LEI mapping files** ([LEI Mapping](https://www.gleif.org/en/lei-data/lei-mapping)) | Published crosswalks: BIC↔LEI, ISIN↔LEI, MIC↔LEI, OpenCorporates↔LEI, S&P CIQ↔LEI. | **Take:** crosswalks are the natural future source of co-reference evidence. **Caution:** they are not all "same-as". ISIN→LEI means *issued by* (an edge), not identity. Evidence must be typed as co-reference only, per category. There is no official CIK↔LEI file, so NVIDIA's link must come from a record that states both. |
| **MusicBrainz merges** ([MBID](https://musicbrainz.org/doc/MusicBrainz_Identifier), [Merge rather than delete](https://musicbrainz.org/doc/Merge_Rather_Than_Delete)) | A merged entity's MBID redirects to the target through redirect tables. "Canonical" mappings are explicitly *not* stable across dumps. | **Take:** old ids resolve through redirects, and clients are told the canonical id may change. **Reject:** physical merge (moving data and deleting the row), which is irreversible. |
| **Wikidata** ([Help:Merge](https://www.wikidata.org/wiki/Help:Merge), [P460 "said to be the same as"](https://www.wikidata.org/wiki/Property:P460)) | P460 records a *claimed*, possibly disputed sameness before merging. A merge leaves a redirect. | **Take:** keep sameness *evidence* separate from the merge *decision*. |
| **Senzing** ([sequence neutrality](https://senzing.com/sequence-neutrality/), [relationship awareness](https://senzing.com/relationship-awareness/)) | Earlier resolutions are re-evaluated as new data arrives. Entities can be "unresolved". The result should not depend on load order. | **Take:** reversibility, and a canonical-id rule that does not depend on evidence order. **Reject:** probabilistic feature matching (out of scope). |
| **Vendor entity masters** (LSEG [PermID](https://developers.lseg.com/en/api-catalog/open-perm-id/permid-entity-search), FactSet [Concordance](https://developer.factset.com/api-catalog/factset-concordance-api) / [Symbology](https://developer.factset.com/api-catalog/symbology-api)) | A proprietary entity id plus concordance (crosswalk) to industry identifiers. Matching usually combines attributes. | **Take:** a stable house id with crosswalks, which is our existing identifier layer. **Reject:** attribute-based matching as identity. |
| **MDM styles** (registry, consolidation, coexistence; golden record; survivorship) | *Registry* keeps source records in place and maintains a cross-reference index. *Consolidation* builds a physical golden record with survivorship rules picking attribute values. | **Take:** the registry style. Sources stay untouched and a link/xref layer sits on top. **Defer:** golden-record attribute survivorship (which name wins). That is policy; v1 shows every member's attributes with provenance. |

Common lesson: every mature system keeps retired identifiers resolvable.
The good ones also keep "somebody said these are the same" apart from "we
treat these as the same".

## 4. Identity evidence

### What counts as evidence

A **co-reference claim** is a source record's statement that a set of two
or more external identifiers, all valid for one node category, identify
the same real-world object, optionally for a validity period.

- It is produced by ingestion whenever one record describes one object
  with ≥2 **primary** identifiers of that category (for entities: LEI and
  CIK). Examples: an SEC submissions document with non-null `lei`, or a
  future GLEIF record carrying a registration-authority id.
- Identifiers are stored as `(scheme, value)`, not as node ids. Sources
  know identifiers, not Undrly nodes, so evidence can exist before, or
  without, any node holding those identifiers. This also covers the
  deferred "claims about nodes that don't exist" case for co-reference.
- Every record carrying the same set yields its own claim. A replay of the
  same record yields the same claim (idempotent). This is how co-reference
  corroboration becomes visible without any ranking: N claims from M
  sources.
- Secondary identifiers (FIGIs, venue symbols) do not produce co-reference
  claims in v1. They remain assignments quarantined on conflict.
- **Never evidence:** names, tickers, exchange names, addresses, fuzzy
  similarity, and "related" crosswalks such as ISIN→LEI, which is an
  issuer relation.

### Evidence vs decision

| | Evidence (`coreference_claims`) | Decision (`identity_links`) |
| --- | --- | --- |
| Says | "record R states {LEI Y, CIK X} are one entity" | "Undrly treats node A and node B as one entity" |
| Author | a source (via its raw record) | a policy (name + version) or an operator |
| Refers to | external identifiers | Undrly nodes |
| Mutability | immutable, never deleted | immutable except one transition, active → retracted |
| Provenance | `source_record_id` (composite FK like other facts) | supporting claim ids, or an operator `source_record_id` |

Operator decisions are made auditable by treating the operator as a source:
an `undrly-operator` source whose raw record holds the decision rationale
(ticket, reasoning). Operator links and retractions then have the same
provenance chain as everything else: link → source_record → source.

## 5. Canonicalization

### Case: CIK → A, LEI → B, then a record states both

1. The record is stored raw (as always) and one co-reference claim
   `{cik X, lei Y}` is stored against it.
2. Entity resolution for that record finds nodes {A, B} that are not in
   one cluster. **Change from today:** this is no longer an error that
   discards the record. Ingestion commits the raw record and the claim,
   mints nothing, assigns nothing new (both identifiers are already
   mapped), and reports `IdentityUnresolved { candidates: [A, B], claim }`.
   The record's other facts are handled as in §9.
3. Separately, the policy (`coreference-v1`) evaluates the claim. If the
   merged cluster would satisfy the invariants (§7), it writes the link
   A–B with the claim as justification. The cluster {A, B} gets canonical
   id `min(A, B)`.
4. Nothing on A or B changes: not the identifiers rows, entity rows, names,
   edges, listings, or any `source_record_id`.

Linking at ingestion time vs by a separate policy step: v1 **separates
them**. Ingestion only records evidence; `undrly-reconcile` decides. The
policy can run right after ingestion, in its own transaction, or in batch.
Ingestion stays policy-free (AGENT.md §12), and a policy bug can never
corrupt source facts.

### Stable ids: options compared

| Option | Mechanism | Pros | Cons |
| --- | --- | --- | --- |
| **A. Survivor = earliest-minted member (recommended)** | Canonical = min UUIDv7 in the cluster; other members point to it (derived). | No new ids. Deterministic and independent of evidence order. A cluster's canonical id changes only when it joins a cluster with an *older* member, and each such change is explained by a link. Matches GLEIF and MusicBrainz successor semantics. Splitting is simple: each component's canonical id is its own oldest member. | A client's stored canonical id can go non-canonical after a merge (still resolvable). The survivor is chosen by age, not by quality. That is acceptable because attribute survivorship is separate. |
| B. Survivor chosen by policy (e.g. the node holding the LEI) | Policy picks the canonical member. | Canonical id can follow "the best" source. | This is source ranking in disguise, so out of scope. The result depends on policy and evidence order. |
| C. New cluster id minted per resolution | Each link mints `undrly:entity:<new>` for the cluster. | Symmetric: no member is privileged. | Mints ids on every merge and split, and every change of the cluster invalidates the cluster id. Merging two clusters that each have ids still needs a survivor rule, so it doesn't avoid option A's problem. Every read goes through indirection. |
| D. No canonical id; the cluster is a set | The API returns the member set. | No survivor choice at all. | Pushes identity work onto every client and gives nothing stable to key on. |
| E. Physical merge (reparent facts, delete or tombstone B) | Classic consolidation MDM. | Simple reads. | Rewrites provenance, is irreversible, and breaks "never reuse or delete ids". **Rejected.** |

**Recommendation: A.** API rule: *any Undrly id ever issued stays valid
forever and returns its own node plus its current canonical id. Clients
that need stability should store the id they received, not assume it is
canonical.*

## 6. Resolution state

The smallest model that works: **one stored state machine, on links only**.
Everything else is derived.

```text
 identity_link:   active ──retract──▶ retracted      (terminal; relinking = new row)
```

The requested states map onto this model as follows:

| Requested name | In this model | Stored? |
| --- | --- | --- |
| unresolved | a node with no active link (singleton cluster) | derived (no rows) |
| linked / proposed | a node pair connected by claims but with no active link. This is a **candidate**. | derived from claims ⋈ identifiers |
| resolved | the nodes are in the same cluster through active links | `identity_links` rows with `retracted_at IS NULL` |
| disputed | claims that, if applied, would violate an invariant (§8) | derived by query and surfaced for review, never stored as a state |
| superseded | a retracted link | `retracted_at IS NOT NULL` |

No stored "proposed" or "disputed" rows. They would be a second copy of
what the claims already say, and they would go stale whenever evidence
changes. A review queue (who looked at what) can be added later as
operator records without changing this model.

## 7. Invariants

1. **Ids are forever.** No `nodes` row is deleted, updated, or reused.
   Resolution never mints, retires, or reissues an id.
2. **Source assertions are untouched.** Resolution never writes to
   `identifiers`, `listing_symbols`, `listings`, `graph_edges`,
   `identifier_conflicts`, category tables, or `source_records`.
3. **Evidence has provenance.** Every co-reference claim has a
   `source_record_id` whose source and receipt time match (composite FK).
   Claims are never updated or deleted.
4. **Decisions have justification.** A policy link has at least one
   supporting claim and a `decided_by` policy name and version. An operator
   link or retraction has an operator `source_record_id`.
5. **Same category only.** A link connects two nodes of the same category
   (entity with entity, instrument with instrument, ...). This is enforced
   by composite FKs to `nodes (id, category)`.
6. **One active link per pair**, stored with `node_a < node_b`. Links are
   append-only apart from a single `active → retracted` transition.
7. **Canonical id = smallest UUIDv7 in the cluster.** `identity_canonical`
   always equals its recomputation from active links; a rebuild function
   plus a test assert this.
8. **Clusters respect one-per-object namespaces.** A cluster must not
   contain two different values of a namespace that identifies at most one
   value per object (LEI, CIK, ISIN, MIC, ISO 4217) with overlapping
   validity. The policy refuses such links and surfaces them as conflicts.
   An operator may override this (GLEIF `DUPLICATE` case), and the override
   is recorded on the link.
9. **Retractions stick.** The policy never re-creates a link that was
   retracted between the same pair unless a new supporting claim was
   stored after the retraction. Only an operator can relink on old
   evidence. Without this rule, the policy would undo reversals.
10. **No weak signals.** Names, tickers, exchanges, addresses and
    similarity scores are never evidence and never policy input.

## 8. Conflicting evidence

Scenario: record R_A (source A) claims `{CIK X, LEI Y}`; record R_B
(source B) claims `{CIK X, LEI Z}`.

Taken together, the claims imply X ≡ Y and X ≡ Z, so Y ≡ Z. That is only a
problem because LEI is one-per-object (invariant 8): one entity cannot have
two different LEIs at the same time, except in the documented GLEIF
duplicate case.

1. **Both claims are stored**, each with its record. Neither is modified.
2. The policy evaluates claims one at a time, in claim id order
   (deterministic):
   - It evaluates R_A's claim first: no violation, so it links nodes(X) and
     nodes(Y).
   - R_B's claim would put LEI Y and LEI Z in one cluster, so the policy
     **does not link**. The conflict query now reports it.
3. **This order-dependence is itself a finding.** The policy must not quietly
   let "first claim wins" become truth. So `coreference-v1` evaluates each
   claim **against all claims that share an identifier with it**, not
   against the current clusters alone. If the union of connected claims
   violates invariant 8, *none* of the contested links is made, and every
   claim involved is reported as a conflict. With both claims present, the
   result is the same in any order (sequence-neutral). If R_A was linked
   before R_B arrived, the policy **does not retract** automatically in
   v1. It reports "active link contradicted by later evidence", and an
   operator decides. (Automatic retraction is a later, explicit policy.)
4. The conflict surface (a repository query, §10) returns the connected
   component of claims, the identifier values that collide, each claim's
   source and record, and any active links involved.

No source is preferred, and nothing is decided silently.

## 9. Interaction with existing graph data

**Principle: facts stay where they were asserted; resolution changes how
they are *read*.**

| Data | After A–B are linked | After retraction |
| --- | --- | --- |
| `identifiers` | LEI stays on A, CIK stays on B. `resolve(any)` goes through the cluster. | unchanged |
| entity rows / names | Both kept. The canonical view shows each member's name with provenance; choosing one is survivorship policy (deferred). | unchanged |
| `graph_edges` (e.g. instrument ISSUED_BY A) | Stored on the original endpoint. The canonical graph view rewrites endpoints to canonical ids at read time and keeps `assertedOn`. Equivalent assertions on A and B are shown as one edge with several assertions. | the view goes back to per-node edges |
| `listings`, `listing_symbols` | stay on their instruments/venues; reached through the cluster in canonical views | unchanged |
| `identifier_conflicts`, `source_records` | untouched | untouched |

### Ingestion changes that come with Phase 5

- **Cluster-aware resolution.** If a record's primary identifiers map to
  nodes that are all in one active cluster, the record resolves to the
  cluster's **canonical** node. It does not error and it does not mint. A
  new primary identifier is assigned only if no cluster member already
  holds it, and it goes to the canonical node.
- **Assigning an identifier already held by another member of the same
  cluster** is reported as `HeldByCluster(member)`, not quarantined as a
  conflict.
- **Several clusters or singletons:** store the raw record and the claim,
  and report `IdentityUnresolved`. For reference records (security + issuer
  + listing), the non-identity facts that depend on the unresolved node
  (e.g. `ISSUED_BY` to the issuer) are **not written** until resolution. The
  record can be replayed after linking. Its raw bytes are already stored,
  so replay is local and exact.

Copying or reparenting historical assertions is rejected. It would
duplicate provenance, make reversal a data migration, and turn the source
fact "record R said X about B" into the fabricated "record R said X about
A".

## 10. Resolution API semantics

Three things are always distinguishable: the **matched node**, the
**canonical identity**, and the **evidence and decisions** connecting them.

### Before resolution (today's NVIDIA state)

```jsonc
// GET /v1/resolve?q=CIK:0001045810
{ "matches": [{
    "nodeId":      "undrly:entity:B",
    "canonicalId": "undrly:entity:B",          // singleton cluster
    "matchedBy":   { "scheme": "cik", "value": "0001045810", "sourceRecord": "r2" },
    "cluster":     { "members": ["undrly:entity:B"], "links": [] },
    "candidates":  []                            // no claims connect B to anything
}]}
```

`resolve(LEI)` returns A in the same way. `get(A)` and `get(B)` each return
their own node with `canonicalId` equal to themselves.

### After evidence, before a decision

`resolve(CIK)` still returns B as canonical, and now includes
`"candidates": [{ "nodeId": "…A", "claims": ["c1"] }]`, so evidence is
visible without being acted on.

### After resolution

```jsonc
// GET /v1/resolve?q=CIK:0001045810
{ "matches": [{
    "nodeId":      "undrly:entity:B",           // what the identifier is attached to
    "canonicalId": "undrly:entity:A",           // min UUIDv7 of {A, B}
    "matchedBy":   { "scheme": "cik", "value": "0001045810", "sourceRecord": "r2" },
    "cluster": {
      "members": ["undrly:entity:A", "undrly:entity:B"],
      "links": [{ "id": "l1", "between": ["…A", "…B"],
                  "decidedBy": "policy:coreference-v1", "claims": ["c1"] }]
    }
}]}
```

- `resolve(LEI)` gives `nodeId: A`, `canonicalId: A`, and the same cluster.
- `get(B)` returns B's own record (name `NVIDIA CORP`, CIK, provenance r2)
  with `canonicalId: A`. It is **not** an HTTP redirect: B is a real,
  permanent node. `?view=canonical` returns the cluster view.
- `get(A)` or `get(A, view=canonical)` returns the union: identifiers from
  A and B, both names, and edges from both. Each item carries `assertedOn`
  and `sourceRecord`.

### After reversal

`canonicalId` for B goes back to B. `cluster.links` is empty. The response
may include `"history": [{ "link": "l1", "retractedBy": "r4" }]`. The
claim c1 still appears as a candidate, but invariant 9 keeps the policy
from relinking on it.

## 11. Database design (proposal, no migration yet)

```sql
-- Which namespaces allow at most one value per object at a time.
ALTER TABLE identifier_schemes
  ADD COLUMN one_per_object boolean NOT NULL DEFAULT true;
-- figi: false (share-class and exchange-level FIGIs coexist on different
-- categories, and one listing may acquire several over time). All others: true.

-- Evidence: record R claims these identifiers identify one object.
CREATE TABLE coreference_claims (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  node_category    text NOT NULL,
  valid_during     validity NOT NULL DEFAULT '(,)',
  -- Sorted "scheme:value|scheme:value", the replay key for the member set.
  member_key       text NOT NULL,
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT coreference_claims_replay_key
    UNIQUE (source_record_id, node_category, member_key, valid_during),
  UNIQUE (id, node_category)
);
CREATE INDEX coreference_claims_source_record_idx ON coreference_claims (source_record_id);

CREATE TABLE coreference_claim_members (
  claim_id      bigint NOT NULL,
  node_category text NOT NULL,
  scheme        text NOT NULL,
  value         text NOT NULL,
  PRIMARY KEY (claim_id, scheme, value),
  FOREIGN KEY (claim_id, node_category) REFERENCES coreference_claims (id, node_category),
  -- The namespace must be able to identify the claim's category.
  FOREIGN KEY (scheme, node_category) REFERENCES identifier_schemes (scheme, node_category)
  -- plus the same per-scheme shape CHECK as identifiers (factored into a
  -- shared IMMUTABLE function identifier_value_is_valid(scheme, value)).
);
CREATE INDEX coreference_claim_members_lookup_idx ON coreference_claim_members (scheme, value);
-- "≥ 2 members": enforced by the repository (a claim and its members are
-- inserted in one transaction). A DEFERRABLE constraint trigger is possible
-- if we want the database to enforce it too.

-- Decisions: Undrly treats node_a and node_b as one object.
CREATE TABLE identity_links (
  id                  bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  node_category       text NOT NULL,
  node_a              uuid NOT NULL,
  node_b              uuid NOT NULL,
  decided_by          text NOT NULL
                        CHECK (decided_by ~ '^(policy:[a-z0-9-]+@[0-9]+|operator)$'),
  decided_at          timestamptz NOT NULL DEFAULT now(),
  decision_record_id  bigint REFERENCES source_records (id),  -- operator rationale
  overrides_uniqueness boolean NOT NULL DEFAULT false,       -- invariant 8 override
  retracted_at        timestamptz,
  retracted_by        text CHECK (retracted_by ~ '^(policy:[a-z0-9-]+@[0-9]+|operator)$'),
  retraction_record_id bigint REFERENCES source_records (id),
  FOREIGN KEY (node_a, node_category) REFERENCES nodes (id, category),
  FOREIGN KEY (node_b, node_category) REFERENCES nodes (id, category),
  CONSTRAINT identity_links_ordered CHECK (node_a < node_b),
  CONSTRAINT identity_links_operator_has_record
    CHECK (decided_by <> 'operator' OR decision_record_id IS NOT NULL),
  CONSTRAINT identity_links_override_is_operator
    CHECK (NOT overrides_uniqueness OR decided_by = 'operator'),
  CONSTRAINT identity_links_retraction_complete CHECK (
    (retracted_at IS NULL) = (retracted_by IS NULL)
    AND (retracted_by IS DISTINCT FROM 'operator' OR retraction_record_id IS NOT NULL))
);
-- One active link per pair; retracted rows stay as history.
CREATE UNIQUE INDEX identity_links_one_active_per_pair
  ON identity_links (node_a, node_b) WHERE retracted_at IS NULL;
CREATE INDEX identity_links_node_b_idx ON identity_links (node_b) WHERE retracted_at IS NULL;
-- Append-only: a trigger rejects DELETE, and rejects UPDATE except setting
-- the retraction columns once (NULL → value).

-- Justification: which claims support a link.
CREATE TABLE identity_link_evidence (
  link_id  bigint NOT NULL REFERENCES identity_links (id),
  claim_id bigint NOT NULL REFERENCES coreference_claims (id),
  PRIMARY KEY (link_id, claim_id)
);
-- "Policy links have ≥ 1 claim": enforced by the repository (same
-- transaction), optionally by a deferred constraint trigger.

-- Derived read cache: only nodes in non-singleton clusters have rows.
-- Absent row ⇒ canonical = self.
CREATE TABLE identity_canonical (
  node_id       uuid PRIMARY KEY,
  node_category text NOT NULL,
  canonical_id  uuid NOT NULL,
  FOREIGN KEY (node_id, node_category) REFERENCES nodes (id, category),
  FOREIGN KEY (canonical_id, node_category) REFERENCES nodes (id, category),
  CONSTRAINT identity_canonical_is_min CHECK (canonical_id <= node_id)
);
CREATE INDEX identity_canonical_canonical_idx ON identity_canonical (canonical_id);
```

Deliberately **not** included: stored proposal or dispute states, a golden
record table, confidence columns, source priority, cluster ids, and any
change to existing tables other than `identifier_schemes.one_per_object`.
Canonical ids compare with `<` on `uuid`, which is UUIDv7 time order
(`nodes_id_check` guarantees v7).

Size note: clusters come from recursive CTEs over active links, only when a
link is written or retracted. Reads use `identity_canonical`, one indexed
lookup.

## 12. Rust design (proposal, no code yet)

### `undrly-core`

```rust
impl Namespace {
    /// At most one value per object at any instant (LEI, CIK, ISIN, MIC,
    /// ISO 4217). Mirrors identifier_schemes.one_per_object.
    pub const fn one_per_object(self) -> bool;
}

/// A source's claim that these identifiers identify one object.
pub struct CoreferenceClaim {
    category: Category,
    identifiers: BTreeSet<ExternalIdentifier>, // ≥ 2, all allowed for `category`
    valid_during: Validity,
    provenance: Provenance,
}
impl CoreferenceClaim {
    pub fn new(category, identifiers, valid_during, provenance)
        -> Result<Self, CoreferenceError>; // fewer than 2, wrong category, or two
                                           // values of one one_per_object namespace
}

/// Who decided a link or a retraction.
pub enum IdentityActor {
    Policy { name: PolicyName, version: u32 },  // e.g. coreference-v1
    Operator,                                   // requires an operator source record
}
```

A claim with two values of one one-per-object namespace (e.g. two LEIs) is
rejected at construction: a single record can't claim that, and the
duplicate-LEI case is an operator decision.

### `undrly-store` (new module `identity`)

| Operation | Returns |
| --- | --- |
| `record_coreference_claim(conn, &CoreferenceClaim, SourceRecordId)` | `(ClaimId, Write)`; a replay is `Unchanged` |
| `claims_for_identifier(conn, &ExternalIdentifier)` | `Vec<StoredClaim>` with `source_record` |
| `claim_component(conn, ClaimId)` | every claim connected through shared identifiers (the policy's input) |
| `link_nodes(conn, a, b, &LinkDecision { actor, claims, decision_record, overrides_uniqueness })` | `LinkOutcome::{Linked(LinkId), AlreadyLinked(LinkId), WouldViolate(Vec<Violation>), SameNode}` |
| `retract_link(conn, LinkId, &Retraction { actor, record })` | `RetractOutcome::{Retracted, AlreadyRetracted}` |
| `canonical_of(conn, CanonicalId)` | `IdentityView { node, canonical, members, active_links }` |
| `links_of(conn, CanonicalId, include_retracted)` | `Vec<StoredLink>` (history) |
| `identity_conflicts(conn, Category)` / `…_for_identifier` | `Vec<IdentityConflict { claims, colliding: Vec<(Namespace, Vec<value>)>, active_links }>` |
| `rebuild_identity_canonical(conn)` | test and repair helper: recompute the cache and compare |

`link_nodes` and `retract_link` are atomic. Each checks invariant 8 over the
resulting cluster(s) with the rows locked, writes the link or retraction,
and recomputes `identity_canonical` for the affected components, all in one
transaction. This is persistence plus invariant enforcement. It contains no
policy.

### `undrly-reconcile` (new crate, per AGENT.md §7)

```rust
pub struct CoreferenceV1;
impl CoreferenceV1 {
    /// Evaluate one claim's component; link when the whole component is
    /// consistent, otherwise report. Never retracts, never overrides.
    pub async fn evaluate(conn, ClaimId) -> Result<PolicyReport, ReconcileError>;
}
```

### `undrly-ingest`

- Entity and reference ingestion record a `CoreferenceClaim` whenever a
  record carries ≥2 primary identifiers for one object.
- Resolution is cluster-aware (§9). The `Ambiguous` error for split
  identifiers becomes the `IdentityUnresolved` outcome, which is committed
  with the raw record and the claim.
- Ingestion never calls the policy. A caller (a CLI or a job) runs
  `CoreferenceV1::evaluate` afterwards.

## 13. NVIDIA walkthrough

Ids are abbreviated. `A` = `undrly:entity:01…` from the fixture (minted
first, so smaller UUIDv7). `B` = the SEC entity, minted later. The future
record r3 is **hypothetical**: for example an SEC submissions document that
one day reports `"lei": "549300S4KLFTLO7GSQ80"`, or any authoritative
source record stating both identifiers.

### State 0: today

```text
source_records   r1 reference-fixture nvda.json            r2 sec-edgar …/CIK0001045810.json
entities         A company "NVIDIA CORPORATION" rec=r1     B company "NVIDIA CORP" rec=r2
identifiers      lei 549300S4KLFTLO7GSQ80 → A rec=r1       cik 0001045810 → B rec=r2
graph_edges      instrument(NVDA common) ISSUED_BY A rec=r1   (SEC asserts no edges)
coreference_*    (none)
identity_links   (none)
identity_canonical (none)       ⇒ canonical(A)=A, canonical(B)=B
```

### State 1: after evidence (r3 ingested)

```text
source_records   + r3 (source S, exact bytes)
coreference_claims         c1 entity valid=(,) member_key="cik:0001045810|lei:549300S4KLFTLO7GSQ80" rec=r3
coreference_claim_members  (c1, cik, 0001045810), (c1, lei, 549300S4KLFTLO7GSQ80)
entities / identifiers / graph_edges   unchanged (both identifiers already mapped; nothing minted)
identity_links, identity_canonical     unchanged (empty)
ingest report    IdentityUnresolved { candidates: [A, B], claim: c1 }
```

`resolve(CIK)` gives B, canonical B, with candidate A via c1.

### State 2: after resolution (`coreference-v1` evaluates c1)

The component of c1 is {c1}. It holds one LEI and one CIK, so invariant 8
holds and the policy links.

```text
identity_links          l1 entity node_a=A node_b=B decided_by=policy:coreference-v1@1 retracted_at=NULL
identity_link_evidence  (l1, c1)
identity_canonical      (A → A), (B → A)
everything else         unchanged
```

`resolve(CIK)` gives node B, canonical A, link l1, claim c1 (record r3).
`resolve(LEI)` gives node A, canonical A. `get(B)` returns B's own data with
canonical A. The canonical graph view of A includes `ISSUED_BY` (asserted
on A, r1), both names, and both identifiers, each with its record.

### State 3: after a hypothetical reversal

Say r3 turns out to be wrong, and an operator retracts with rationale record
r4 (source `undrly-operator`):

```text
source_records   + r4 (undrly-operator, rationale payload)
identity_links   l1 … retracted_at=T retracted_by=operator retraction_record_id=r4
identity_canonical  (rows for A, B deleted) ⇒ canonical(A)=A, canonical(B)=B
coreference_claims  c1 still present (evidence is history)
everything else     unchanged
```

`CoreferenceV1::evaluate(c1)` now makes **no** link (invariant 9: retracted
pair, no newer claim). A new record r5 with a new claim c2 would allow the
policy to reconsider. c2 would count as new evidence, visible in the
history, and the relink would be a new row l2.

In every state, A and B exist, their ids are unchanged, and each
identifier, edge and name still names its original record (r1 or r2).

## 14. Scope for Phase 5 implementation (when approved)

1. Core: `Namespace::one_per_object`, `CoreferenceClaim`, `IdentityActor`.
2. Store: migration for the four tables plus the `identifier_schemes`
   column, the append-only trigger, the `identity` repository module, and
   the rebuild check.
3. Ingest: record claims, cluster-aware resolution, the
   `IdentityUnresolved` outcome.
4. Reconcile: the `undrly-reconcile` crate with `CoreferenceV1`.
5. Tests:
   - the NVIDIA lifecycle above, driven by a synthetic r3 fixture (clearly
     labelled synthetic);
   - conflict order-independence;
   - retraction stickiness;
   - cache equals recomputation;
   - no writes to source tables during link or retract;
   - operator override for a duplicate LEI.

Out of scope: source ranking, confidence, fuzzy or ML matching, golden
record attribute survivorship, automatic retraction, instrument or listing
co-reference sources (the model supports them, but no source provides
them yet), and API endpoints.

## 15. Open questions for review

1. **Canonical rule.** Earliest-minted (recommended) vs something else. The
   rule is the only survivorship decision v1 makes.
2. **Policy trigger.** Run `coreference-v1` right after each ingestion
   (separate transaction) or only in batch? The recommendation is right
   after ingestion, so single-record flows resolve without an extra step.
3. **Is one record enough to link?** v1 links on one consistent claim, the
   same trust ingestion already gives one record for an identifier
   assignment. A "≥2 independent sources" knob belongs to a later policy
   version.
4. **Held-back facts.** When a reference record is `IdentityUnresolved`,
   should edges to the *resolved* parts (e.g. `DENOMINATED_IN`) still be
   written, with only the edges touching the unresolved node held back? The
   proposal holds back only facts that depend on the unresolved node.
5. **CIK as one-per-object.** SEC occasionally has several CIKs for related
   filers. Treating CIK as one-per-object is conservative: it creates
   conflicts rather than merges. Confirm.
