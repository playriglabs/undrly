-- Cross-market v1 (docs/hackathon-v1.md): commodities and perpetuals,
-- observations of any priced subject with provenance, search aliases, quote
-- feeds, and canonical quotes.

-- Instrument classes. Must equal undrly_core::InstrumentClass.
ALTER TABLE instruments
  DROP CONSTRAINT instruments_instrument_class_check,
  ADD CONSTRAINT instruments_instrument_class_check
    CHECK (instrument_class IN ('equity', 'crypto_asset', 'commodity', 'perpetual_future'));

-- A derivative (e.g. a perpetual) derives from its underlying instrument.
INSERT INTO relationship_rules (relationship_type, subject_category, object_category)
  VALUES ('DERIVES_FROM', 'instrument', 'instrument');

-- Observations are reshaped: nothing ever wrote the 0005 table, so it is
-- replaced rather than migrated.
--
-- A subject is what is priced: an instrument, or a fiat currency (FX: 1 EUR
-- in USD). `observed_at` is the source's time and is NULL when the source
-- states none; it is never filled in with Undrly's clock. Every observation
-- names the raw record it was normalized from.
DROP TABLE market_observations;

CREATE TABLE market_observations (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  basis            text NOT NULL CHECK (basis IN ('venue', 'aggregated', 'derived')),
  venue_id         uuid REFERENCES venues (id),
  price_type       text NOT NULL CHECK (price_type IN ('last', 'mid', 'mark', 'reference')),
  price            financial_decimal NOT NULL,
  bid              financial_decimal,
  ask              financial_decimal,
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  source_id        source_id NOT NULL,
  observed_at      timestamptz,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT market_observations_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  -- A venue quote names its venue; aggregated/derived values never do.
  CONSTRAINT market_observations_basis_venue CHECK ((basis = 'venue') = (venue_id IS NOT NULL)),
  CONSTRAINT market_observations_not_self_denominated CHECK (unit_id <> subject_id),
  CONSTRAINT market_observations_bid_ask
    CHECK ((bid IS NULL) = (ask IS NULL) AND (bid IS NULL OR bid <= ask)),
  -- Replaying a record yields the same observations.
  CONSTRAINT market_observations_record_key UNIQUE NULLS NOT DISTINCT
    (source_record_id, subject_id, unit_id, venue_id, price_type),
  -- Lets canonical_quotes reference an observation of the same pair.
  UNIQUE (id, subject_id, unit_id)
);
-- A new response restating a price the source already timestamped is the same
-- observation (e.g. an unchanged reference price).
CREATE UNIQUE INDEX market_observations_replay_key
  ON market_observations (source_id, subject_id, unit_id, basis, venue_id, price_type, observed_at)
  NULLS NOT DISTINCT
  WHERE observed_at IS NOT NULL;
CREATE INDEX market_observations_pair_idx
  ON market_observations (subject_id, unit_id, source_id, received_at DESC);
CREATE INDEX market_observations_source_record_idx ON market_observations (source_record_id);

-- Search terms (symbols and names) for any node. For discovery only: an alias
-- never selects a node for identity resolution.
CREATE TABLE aliases (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  node_id          uuid NOT NULL,
  node_category    text NOT NULL,
  alias            display_name NOT NULL,
  alias_key        text GENERATED ALWAYS AS (lower(alias)) STORED,
  kind             text NOT NULL CHECK (kind IN ('symbol', 'name')),
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (node_id, node_category) REFERENCES nodes (id, category),
  CONSTRAINT aliases_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT aliases_one_per_source UNIQUE (node_id, alias_key, kind, source_id)
);
CREATE INDEX aliases_key_idx ON aliases (alias_key text_pattern_ops);
CREATE INDEX aliases_source_record_idx ON aliases (source_record_id);

-- A quote feed: source `feed_source_id` publishes, under `symbol`, the
-- `price_type` of `subject` in `unit`, at a venue or aggregated. The feed
-- declaration itself is a fact with provenance (e.g. from curated data).
CREATE TABLE quote_feeds (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  feed_source_id   source_id NOT NULL REFERENCES sources (id),
  symbol           text NOT NULL
                     CHECK (symbol <> '' AND symbol !~ '[[:space:][:cntrl:]]' AND char_length(symbol) <= 64),
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  basis            text NOT NULL CHECK (basis IN ('venue', 'aggregated', 'derived')),
  venue_id         uuid REFERENCES venues (id),
  price_type       text NOT NULL CHECK (price_type IN ('last', 'mid', 'mark', 'reference')),
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT quote_feeds_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT quote_feeds_basis_venue CHECK ((basis = 'venue') = (venue_id IS NOT NULL)),
  CONSTRAINT quote_feeds_not_self_denominated CHECK (unit_id <> subject_id),
  CONSTRAINT quote_feeds_one_per_symbol UNIQUE (feed_source_id, symbol, price_type)
);
CREATE INDEX quote_feeds_pair_idx ON quote_feeds (subject_id, unit_id);
CREATE INDEX quote_feeds_source_record_idx ON quote_feeds (source_record_id);

-- Canonical quotes: derived cache, one row per (subject, unit), pointing at
-- the observation the aggregation method selected. Rewritten by the
-- collector after each ingestion; read by the API without contacting any
-- upstream. Freshness is computed at read time.
CREATE TABLE canonical_quotes (
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  observation_id   bigint NOT NULL,
  method           text NOT NULL CHECK (method IN ('latest-observation-v1')),
  eligible_count   integer NOT NULL CHECK (eligible_count >= 1),
  computed_at      timestamptz NOT NULL,
  PRIMARY KEY (subject_id, unit_id),
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  -- The selected observation prices this very pair.
  FOREIGN KEY (observation_id, subject_id, unit_id)
    REFERENCES market_observations (id, subject_id, unit_id)
);
