-- Multi-source aggregation (BTC/USD milestone).
--
-- A canonical quote is now the output of a named aggregation method, not
-- necessarily one observation: it stores its own price, and
-- `canonical_quote_inputs` records exactly which observations (and so which
-- raw source records) produced it. Observations are never copied or changed.
--
-- canonical_quotes is a derived cache that the collector rebuilds, so it is
-- replaced rather than migrated.

-- Which method aggregates a pair. A declared fact with provenance (curated
-- reference data). Pairs without a row use 'latest-observation-v1'.
CREATE TABLE quote_aggregations (
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  method           text NOT NULL CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1')),
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  PRIMARY KEY (subject_id, unit_id),
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT quote_aggregations_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT quote_aggregations_not_self_denominated CHECK (unit_id <> subject_id)
);
CREATE INDEX quote_aggregations_source_record_idx ON quote_aggregations (source_record_id);

DROP TABLE canonical_quotes;

CREATE TABLE canonical_quotes (
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  method           text NOT NULL CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1')),
  price            financial_decimal NOT NULL,
  price_type       text NOT NULL CHECK (price_type IN ('last', 'mid', 'mark', 'reference')),
  basis            text NOT NULL CHECK (basis IN ('venue', 'aggregated', 'derived')),
  venue_id         uuid REFERENCES venues (id),
  -- The oldest effective time (source time, else receipt time) of the inputs.
  as_of            timestamptz NOT NULL,
  eligible_count   integer NOT NULL CHECK (eligible_count >= 1),
  computed_at      timestamptz NOT NULL,
  PRIMARY KEY (subject_id, unit_id),
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT canonical_quotes_basis_venue CHECK ((basis = 'venue') = (venue_id IS NOT NULL)),
  -- An aggregate of venue mids is never attributed to one venue.
  CONSTRAINT canonical_quotes_mean_is_aggregated CHECK (
    method <> 'mean-venue-mid-v1' OR (basis = 'aggregated' AND price_type = 'mid')
  )
);

-- Exactly the observations a canonical quote was computed from, each with
-- the price it contributed (e.g. its mid).
CREATE TABLE canonical_quote_inputs (
  subject_id     uuid NOT NULL,
  unit_id        uuid NOT NULL,
  observation_id bigint NOT NULL,
  input_price    financial_decimal NOT NULL,
  PRIMARY KEY (subject_id, unit_id, observation_id),
  FOREIGN KEY (subject_id, unit_id) REFERENCES canonical_quotes (subject_id, unit_id)
    ON DELETE CASCADE,
  -- An input prices the very pair it aggregates.
  FOREIGN KEY (observation_id, subject_id, unit_id)
    REFERENCES market_observations (id, subject_id, unit_id)
);
CREATE INDEX canonical_quote_inputs_observation_idx ON canonical_quote_inputs (observation_id);
