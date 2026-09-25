-- V1.1 universe expansion (docs/v1.1-universe.md). All additive.

-- Universe membership: "belongs to U as of T", separate from existence.
-- A snapshot names the upstream record that asserted it; membership never
-- affects canonical identity.
CREATE TABLE universe_snapshots (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  universe_key     text NOT NULL
                     CHECK (universe_key IN ('crypto-top100', 'sp500', 'nasdaq100', 'hyperliquid-perps')),
  as_of            timestamptz NOT NULL,
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  CONSTRAINT universe_snapshots_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT universe_snapshots_replay_key UNIQUE (universe_key, source_record_id)
);
CREATE INDEX universe_snapshots_latest_idx ON universe_snapshots (universe_key, as_of DESC, id DESC);

CREATE TABLE universe_members (
  snapshot_id   bigint NOT NULL REFERENCES universe_snapshots (id),
  node_id       uuid NOT NULL,
  node_category text NOT NULL,
  rank          integer CHECK (rank >= 1),
  source_symbol text CHECK (source_symbol <> '' AND char_length(source_symbol) <= 64),
  PRIMARY KEY (snapshot_id, node_id),
  FOREIGN KEY (node_id, node_category) REFERENCES nodes (id, category)
);
CREATE INDEX universe_members_node_idx ON universe_members (node_id);

-- Feed cadence: after how long a feed's latest observation is stale. The
-- default keeps every V1 feed's behaviour.
ALTER TABLE quote_feeds
  ADD COLUMN stale_after_seconds integer NOT NULL DEFAULT 300 CHECK (stale_after_seconds > 0);

-- Contract terms and commodity units. Must equal undrly_core::UnitOfMeasure.
ALTER TABLE instruments
  ADD COLUMN contract_multiplier financial_decimal CHECK (contract_multiplier > 0),
  ADD COLUMN unit_of_measure text
    CHECK (unit_of_measure IN ('troy_ounce', 'barrel', 'mmbtu', 'metric_ton', 'kilogram'));

-- Price type `average`: a published average over a period (e.g. monthly).
ALTER TABLE market_observations DROP CONSTRAINT market_observations_price_type_check,
  ADD CONSTRAINT market_observations_price_type_check
    CHECK (price_type IN ('last', 'mid', 'mark', 'reference', 'average'));
ALTER TABLE quote_feeds DROP CONSTRAINT quote_feeds_price_type_check,
  ADD CONSTRAINT quote_feeds_price_type_check
    CHECK (price_type IN ('last', 'mid', 'mark', 'reference', 'average'));
ALTER TABLE canonical_quotes DROP CONSTRAINT canonical_quotes_price_type_check,
  ADD CONSTRAINT canonical_quotes_price_type_check
    CHECK (price_type IN ('last', 'mid', 'mark', 'reference', 'average'));
