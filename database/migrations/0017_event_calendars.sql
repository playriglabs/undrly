-- Economic release calendar and earnings calendar (docs/v1.3-market-data.md
-- §10). Additive.

-- Scheduled release dates of curated economic releases, as one fetch stated
-- them. A schedule can change (a date moves or is cancelled), so each fetch
-- is its own snapshot for the window it covered (`economic_calendar_windows`),
-- and a date is read from the newest snapshot covering it.
CREATE TABLE economic_calendar_windows (
  source_record_id bigint PRIMARY KEY,
  source_id        source_id NOT NULL,
  received_at      timestamptz NOT NULL,
  -- The source's release id (FRED `release_id`) the fetch asked for.
  release_key      text NOT NULL CHECK (release_key <> '' AND char_length(release_key) <= 64),
  -- Undrly's category for the release (from its curated list).
  category         text NOT NULL CHECK (category IN
                     ('inflation', 'labor', 'growth', 'consumption', 'production', 'housing',
                      'sentiment', 'trade')),
  window_from      date NOT NULL,
  window_to        date NOT NULL CHECK (window_to >= window_from),
  CONSTRAINT economic_calendar_windows_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at)
);
CREATE INDEX economic_calendar_windows_idx
  ON economic_calendar_windows (release_key, window_from, window_to, received_at DESC);

CREATE TABLE economic_release_dates (
  source_record_id bigint NOT NULL REFERENCES economic_calendar_windows (source_record_id),
  release_key      text NOT NULL,
  release_name     display_name NOT NULL,
  release_date     date NOT NULL,
  PRIMARY KEY (source_record_id, release_key, release_date)
);
CREATE INDEX economic_release_dates_date_idx ON economic_release_dates (release_date);

-- A company's earnings report for one fiscal quarter, as the source states
-- it (date, time of day, estimates and actuals). A newer record replaces a
-- report whose date or figures changed.
CREATE TABLE earnings_events (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  instrument_id    uuid NOT NULL REFERENCES instruments (id),
  source_id        source_id NOT NULL,
  fiscal_year      integer NOT NULL CHECK (fiscal_year BETWEEN 1900 AND 2200),
  fiscal_quarter   integer NOT NULL CHECK (fiscal_quarter BETWEEN 1 AND 4),
  report_date      date NOT NULL,
  -- `before_open` / `after_close` / `during_hours`; NULL when not stated.
  report_time      text CHECK (report_time IN ('before_open', 'after_close', 'during_hours')),
  eps_estimate     financial_decimal,
  eps_actual       financial_decimal,
  revenue_estimate financial_decimal,
  revenue_actual   financial_decimal,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  CONSTRAINT earnings_events_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT earnings_events_one_per_quarter
    UNIQUE (source_id, instrument_id, fiscal_year, fiscal_quarter)
);
CREATE INDEX earnings_events_instrument_idx ON earnings_events (instrument_id, report_date);
CREATE INDEX earnings_events_date_idx ON earnings_events (report_date);
