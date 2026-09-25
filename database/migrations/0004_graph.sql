-- Graph edges in canonical direction only (dependent -> thing it depends on).
-- Inverse traversal (e.g. UNDERLYING_OF) is derived at query time.

-- Storable relationship types and their endpoint categories.
-- Must equal undrly_core::RelationshipType::allowed_endpoints (tested by
-- undrly-store). Types without rows (UNDERLYING_OF, LISTED_ON, and types whose
-- node categories do not exist yet) cannot be stored.
CREATE TABLE relationship_rules (
  relationship_type text NOT NULL,
  subject_category  text NOT NULL,
  object_category   text NOT NULL,
  PRIMARY KEY (relationship_type, subject_category, object_category)
);
INSERT INTO relationship_rules (relationship_type, subject_category, object_category) VALUES
  ('ISSUED_BY',      'instrument', 'entity'),
  ('TRADES_ON',      'instrument', 'venue'),
  ('DENOMINATED_IN', 'instrument', 'currency'),
  ('DENOMINATED_IN', 'instrument', 'instrument'),
  ('SETTLES_IN',     'instrument', 'currency'),
  ('SETTLES_IN',     'instrument', 'instrument');

-- One row per (edge, asserting source, period). Rows are current assertions
-- by a source, not eternal historical truth.
--
-- Edge validity periods are deferred. `valid_during` exists so the uniqueness
-- rule is already "no overlapping periods" rather than "one row per edge and
-- source": a source may later assert the same edge for several disjoint
-- periods without changing this constraint. Until validity is introduced,
-- graph_edges_validity_deferred pins every row to an unbounded period.
CREATE TABLE graph_edges (
  id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  subject_id        uuid NOT NULL,
  subject_category  text NOT NULL,
  relationship_type text NOT NULL,
  object_id         uuid NOT NULL,
  object_category   text NOT NULL,
  valid_during      validity NOT NULL DEFAULT '(,)',
  source_id         source_id NOT NULL REFERENCES sources (id),
  received_at       timestamptz NOT NULL,
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (object_id, object_category) REFERENCES nodes (id, category),
  FOREIGN KEY (relationship_type, subject_category, object_category)
    REFERENCES relationship_rules (relationship_type, subject_category, object_category),
  CONSTRAINT graph_edges_no_self_edge CHECK (subject_id <> object_id),
  CONSTRAINT graph_edges_validity_deferred CHECK (lower_inf(valid_during) AND upper_inf(valid_during)),
  CONSTRAINT graph_edges_one_assertion_per_period EXCLUDE USING gist (
    subject_id WITH =,
    relationship_type WITH =,
    object_id WITH =,
    source_id WITH =,
    valid_during WITH &&
  )
);
-- Forward traversal.
CREATE INDEX graph_edges_subject_idx ON graph_edges (subject_id, relationship_type);
-- Inverse traversal, derived at query time.
CREATE INDEX graph_edges_object_idx ON graph_edges (object_id, relationship_type);
