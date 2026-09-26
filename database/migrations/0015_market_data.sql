-- V1.3 market data surface (docs/v1.3-market-data.md). All additive.
--
-- Historical bars, perpetual contexts, funding schedules and trading
-- sessions each get their own table: they are different facts with
-- different keys, not more rows of `market_observations` (which stays one
-- source's price of a subject at one time). Every row names the raw source
-- record it was normalized from. Canonical financial values are
-- `financial_decimal` columns, never JSON.

-- OHLC bars as a venue publishes them (trade prices). One row per
-- (market, source, venue, interval, open time). A bar still in progress when
-- fetched is replaced by a later fetch of the same bar (the newer record wins);
-- `close_time <= received_at` tells whether the stored bar was complete.
CREATE TABLE market_bars (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category = 'instrument'),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  source_id        source_id NOT NULL,
  venue_id         uuid NOT NULL REFERENCES venues (id),
  -- Intervals stored as the source publishes them (derived ones are not stored).
  bar_interval     text NOT NULL CHECK (bar_interval IN ('1h', '1d')),
  open_time        timestamptz NOT NULL,
  close_time       timestamptz NOT NULL,
  open             financial_decimal NOT NULL,
  high             financial_decimal NOT NULL,
  low              financial_decimal NOT NULL,
  close            financial_decimal NOT NULL,
  -- Quantity of the subject traded (coins, shares, contracts, base currency);
  -- NULL when the source states none.
  volume           financial_decimal CHECK (volume >= 0),
  trade_count      bigint CHECK (trade_count >= 0),
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT market_bars_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT market_bars_not_self_denominated CHECK (unit_id <> subject_id),
  CONSTRAINT market_bars_time CHECK (close_time > open_time),
  CONSTRAINT market_bars_ohlc CHECK (
    low <= high AND low <= open AND open <= high AND low <= close AND close <= high
  ),
  CONSTRAINT market_bars_one_per_period
    UNIQUE (subject_id, unit_id, source_id, venue_id, bar_interval, open_time)
);
-- `/v1/candles`: the latest N bars of one market and interval.
CREATE INDEX market_bars_series_idx
  ON market_bars (subject_id, unit_id, bar_interval, open_time DESC);
CREATE INDEX market_bars_source_record_idx ON market_bars (source_record_id);

-- A perpetual's market context as its venue reports it, one row per record.
-- Units: prices in the perpetual's price unit; `funding_rate` a fraction per
-- `funding_interval_hours`; `open_interest` and `volume_24h_base` in units of
-- the perpetual's underlying (per contract multiplier); `volume_24h_notional`
-- in the price unit.
CREATE TABLE perp_contexts (
  id                     bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  subject_id             uuid NOT NULL REFERENCES instruments (id),
  unit_id                uuid NOT NULL,
  unit_category          text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  source_id              source_id NOT NULL,
  venue_id               uuid NOT NULL REFERENCES venues (id),
  mark_price             financial_decimal NOT NULL CHECK (mark_price > 0),
  oracle_price           financial_decimal CHECK (oracle_price > 0),
  mid_price              financial_decimal CHECK (mid_price > 0),
  funding_rate           financial_decimal,
  funding_interval_hours integer CHECK (funding_interval_hours > 0),
  open_interest          financial_decimal CHECK (open_interest >= 0),
  volume_24h_base        financial_decimal CHECK (volume_24h_base >= 0),
  volume_24h_notional    financial_decimal CHECK (volume_24h_notional >= 0),
  price_24h_ago          financial_decimal CHECK (price_24h_ago > 0),
  received_at            timestamptz NOT NULL,
  source_record_id       bigint NOT NULL,
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT perp_contexts_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT perp_contexts_funding_interval
    CHECK ((funding_rate IS NULL) = (funding_interval_hours IS NULL)),
  CONSTRAINT perp_contexts_record_key UNIQUE (source_record_id, subject_id)
);
CREATE INDEX perp_contexts_latest_idx ON perp_contexts (subject_id, received_at DESC);

-- A venue's trading sessions by local trading date (a date without a row,
-- inside the loaded range, has no session). Times are absolute instants.
CREATE TABLE trading_sessions (
  venue_id         uuid NOT NULL REFERENCES venues (id),
  session_date     date NOT NULL,
  pre_open_at      timestamptz NOT NULL,
  open_at          timestamptz NOT NULL,
  close_at         timestamptz NOT NULL,
  post_close_at    timestamptz NOT NULL,
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  PRIMARY KEY (venue_id, session_date),
  CONSTRAINT trading_sessions_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT trading_sessions_order
    CHECK (pre_open_at <= open_at AND open_at < close_at AND close_at <= post_close_at)
);

-- Which dates a venue's calendar has been loaded for (a date outside every
-- loaded range has an unknown session).
CREATE TABLE trading_calendar_ranges (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  venue_id         uuid NOT NULL REFERENCES venues (id),
  first_date       date NOT NULL,
  last_date        date NOT NULL CHECK (last_date >= first_date),
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  CONSTRAINT trading_calendar_ranges_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT trading_calendar_ranges_record_key UNIQUE (source_record_id, venue_id)
);

-- Reference series history (`/v1/history`): the observations of one feed,
-- newest first.
CREATE INDEX market_observations_series_idx
  ON market_observations (subject_id, unit_id, source_id, observed_at DESC)
  WHERE observed_at IS NOT NULL;

-- A reference-series payload (e.g. 90 days of ECB rates) yields one
-- observation per date for the same pair; replaying it still yields the same
-- rows. The record key therefore includes the source's time.
ALTER TABLE market_observations
  DROP CONSTRAINT market_observations_record_key,
  ADD CONSTRAINT market_observations_record_key UNIQUE NULLS NOT DISTINCT
    (source_record_id, subject_id, unit_id, venue_id, price_type, observed_at);
