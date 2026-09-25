-- Source-record provenance: every source-derived fact names the exact raw
-- record that asserted it.
--
--   fact --source_record_id--> source_records --source_id--> sources
--
-- `source_record_id` is the *originating* record: the one whose ingestion
-- wrote the row. A later record that asserts the same fact leaves the row
-- unchanged and is not recorded as supporting evidence (corroboration is
-- deferred to reconciliation). Supporting assertions can be added later as a
-- separate (fact, source_record) table without changing this column.
--
-- Where a fact also stores `source_id` and `received_at`, a composite foreign
-- key makes them equal to its record's, so the three can never disagree.
-- Payloads are not copied; they stay in `source_records`.
--
-- Market observations are not covered: they are not produced by ingestion
-- yet, and raw-record retention for high-volume feeds is undecided.

-- Facts written before this migration cannot be traced to a record, and
-- inventing a link would be worse than none. No such rows exist outside tests.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM entities) OR EXISTS (SELECT 1 FROM instruments)
     OR EXISTS (SELECT 1 FROM venues) OR EXISTS (SELECT 1 FROM currencies)
     OR EXISTS (SELECT 1 FROM listings) OR EXISTS (SELECT 1 FROM identifiers)
     OR EXISTS (SELECT 1 FROM listing_symbols) OR EXISTS (SELECT 1 FROM graph_edges)
     OR EXISTS (SELECT 1 FROM identifier_conflicts) THEN
    RAISE EXCEPTION '0007_source_record_provenance requires empty fact tables: existing facts have no source record';
  END IF;
END $$;

-- Target for composite provenance foreign keys.
ALTER TABLE source_records
  ADD CONSTRAINT source_records_provenance_key UNIQUE (id, source_id, received_at);

-- Node objects: the record whose ingestion minted the node and supplied its
-- attributes (name, kind, class). Later records never modify the object.
ALTER TABLE entities
  ADD COLUMN source_record_id bigint NOT NULL
    CONSTRAINT entities_source_record_fkey REFERENCES source_records (id);
ALTER TABLE instruments
  ADD COLUMN source_record_id bigint NOT NULL
    CONSTRAINT instruments_source_record_fkey REFERENCES source_records (id);
ALTER TABLE venues
  ADD COLUMN source_record_id bigint NOT NULL
    CONSTRAINT venues_source_record_fkey REFERENCES source_records (id);
ALTER TABLE currencies
  ADD COLUMN source_record_id bigint NOT NULL
    CONSTRAINT currencies_source_record_fkey REFERENCES source_records (id);

ALTER TABLE listings
  ADD COLUMN source_record_id bigint NOT NULL,
  ADD CONSTRAINT listings_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at);

ALTER TABLE identifiers
  ADD COLUMN source_record_id bigint NOT NULL,
  ADD CONSTRAINT identifiers_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at);

ALTER TABLE listing_symbols
  ADD COLUMN source_record_id bigint NOT NULL,
  ADD CONSTRAINT listing_symbols_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at);

-- Graph edges stay one row per (edge, source, period). Contradictory
-- assertions (e.g. two ISSUED_BY objects for one instrument) are separate rows,
-- each naming its record; none is chosen as canonical until reconciliation.
ALTER TABLE graph_edges
  ADD COLUMN source_record_id bigint NOT NULL,
  ADD CONSTRAINT graph_edges_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at);

-- A quarantined claim names the record that made it. The same claim from a
-- different record is separate evidence and gets its own row; replaying the
-- same record still adds nothing.
ALTER TABLE identifier_conflicts
  ADD COLUMN source_record_id bigint NOT NULL,
  ADD CONSTRAINT identifier_conflicts_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  DROP CONSTRAINT identifier_conflicts_replay_key,
  ADD CONSTRAINT identifier_conflicts_replay_key UNIQUE NULLS NOT DISTINCT (
    namespace, value, scope_venue_id, claimed_node_id, claimed_valid_during,
    source_id, conflicting_identifier_id, conflicting_listing_symbol_id,
    source_record_id
  );

-- "What did this record assert?"
CREATE INDEX entities_source_record_idx ON entities (source_record_id);
CREATE INDEX instruments_source_record_idx ON instruments (source_record_id);
CREATE INDEX venues_source_record_idx ON venues (source_record_id);
CREATE INDEX currencies_source_record_idx ON currencies (source_record_id);
CREATE INDEX listings_source_record_idx ON listings (source_record_id);
CREATE INDEX identifiers_source_record_idx ON identifiers (source_record_id);
CREATE INDEX listing_symbols_source_record_idx ON listing_symbols (source_record_id);
CREATE INDEX graph_edges_source_record_idx ON graph_edges (source_record_id);
CREATE INDEX identifier_conflicts_source_record_idx ON identifier_conflicts (source_record_id);
