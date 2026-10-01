-- A reference series' open/high/low/close over a window, as its source
-- states it (docs/v1.9-live-fx.md). gold-api's `/ohlc` over the 24 hours
-- before each poll gives the metals a 24-hour change from the source itself;
-- the window is the source's, never aligned or resampled by Undrly. Not a
-- bar: `market_bars` holds a venue's own aligned intervals.
CREATE TABLE reference_windows (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  subject_id       uuid NOT NULL,
  subject_category text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id          uuid NOT NULL,
  unit_category    text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  source_id        source_id NOT NULL,
  window_start     timestamptz NOT NULL,
  window_end       timestamptz NOT NULL CHECK (window_end > window_start),
  open             financial_decimal NOT NULL,
  high             financial_decimal NOT NULL,
  low              financial_decimal NOT NULL,
  close            financial_decimal NOT NULL,
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  CONSTRAINT reference_windows_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT reference_windows_ohlc_order CHECK (
    low <= open AND open <= high AND low <= close AND close <= high
  ),
  UNIQUE (subject_id, unit_id, source_id, window_start, window_end)
);
CREATE INDEX reference_windows_latest_idx
  ON reference_windows (subject_id, unit_id, window_end DESC);
CREATE INDEX reference_windows_source_record_idx ON reference_windows (source_record_id);
