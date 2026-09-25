-- Raw source records and idempotent conflict quarantine (Phase 3).

-- Raw payloads exactly as received, for provenance and audit. Stored as bytes,
-- not parsed JSON: a raw record is evidence from a source, not canonical data.
-- Replaying the same payload for the same record key is a no-op.
CREATE TABLE source_records (
  id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  source_id      source_id NOT NULL REFERENCES sources (id),
  record_key     text NOT NULL
                   CHECK (record_key <> '' AND char_length(record_key) <= 256 AND record_key !~ '[[:cntrl:]]'),
  payload        bytea NOT NULL,
  payload_sha256 bytea GENERATED ALWAYS AS (sha256(payload)) STORED,
  received_at    timestamptz NOT NULL,
  CONSTRAINT source_records_replay_key UNIQUE (source_id, record_key, payload_sha256)
);

-- Replaying the same conflicting claim must not add duplicate quarantine rows.
-- received_at / detected_at are excluded: a replay is the same claim.
ALTER TABLE identifier_conflicts
  ADD CONSTRAINT identifier_conflicts_replay_key UNIQUE NULLS NOT DISTINCT (
    namespace, value, scope_venue_id, claimed_node_id, claimed_valid_during,
    source_id, conflicting_identifier_id, conflicting_listing_symbol_id
  );
