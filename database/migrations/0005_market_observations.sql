-- Normalized market observations. Prices are exact decimals with scale
-- preserved; the unit is a canonical currency or asset node, never a code.

CREATE TABLE market_observations (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  instrument_id uuid NOT NULL REFERENCES instruments (id),
  basis         text NOT NULL CHECK (basis IN ('venue', 'aggregated', 'derived')),
  venue_id      uuid REFERENCES venues (id),
  price         financial_decimal NOT NULL,
  unit_id       uuid NOT NULL,
  unit_category text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  source_id     source_id NOT NULL REFERENCES sources (id),
  observed_at   timestamptz NOT NULL,
  received_at   timestamptz NOT NULL,
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  -- A venue quote names its venue; aggregated/derived values never do.
  CONSTRAINT market_observations_basis_venue CHECK ((basis = 'venue') = (venue_id IS NOT NULL)),
  CONSTRAINT market_observations_not_self_denominated CHECK (unit_id <> instrument_id),
  -- Idempotent replay: one price per source, instrument, market, unit, instant.
  CONSTRAINT market_observations_replay_key UNIQUE NULLS NOT DISTINCT
    (source_id, instrument_id, basis, venue_id, unit_id, observed_at)
);
CREATE INDEX market_observations_latest_idx
  ON market_observations (instrument_id, observed_at DESC);
CREATE INDEX market_observations_source_received_idx
  ON market_observations (source_id, received_at DESC);
