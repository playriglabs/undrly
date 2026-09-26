-- V1.2 FX universes (docs/v1.2-fx.md). All additive except the V1 EUR/USD
-- re-pointing at the end.

-- An FX market is an instrument (class `fx`) between two currency nodes. The
-- currencies stay `currencies`; only the market is an instrument. Price =
-- units of the quote currency per one unit of the base currency.
ALTER TABLE instruments
  DROP CONSTRAINT instruments_instrument_class_check,
  ADD CONSTRAINT instruments_instrument_class_check
    CHECK (instrument_class IN ('equity', 'crypto_asset', 'commodity', 'perpetual_future', 'fx')),
  ADD COLUMN base_currency_id uuid REFERENCES currencies (id),
  ADD COLUMN quote_currency_id uuid REFERENCES currencies (id),
  -- Exactly FX instruments state both currencies, and they differ.
  ADD CONSTRAINT instruments_fx_pair CHECK (
    (instrument_class = 'fx') = (base_currency_id IS NOT NULL)
    AND (base_currency_id IS NULL) = (quote_currency_id IS NULL)
    AND (base_currency_id IS NULL OR base_currency_id <> quote_currency_id)
  ),
  -- One FX instrument per oriented pair: a pair in two universes is one node.
  ADD CONSTRAINT instruments_fx_pair_unique UNIQUE (base_currency_id, quote_currency_id);

ALTER TABLE universe_snapshots
  DROP CONSTRAINT universe_snapshots_universe_key_check,
  ADD CONSTRAINT universe_snapshots_universe_key_check
    CHECK (universe_key IN ('crypto-top100', 'sp500', 'nasdaq100', 'hyperliquid-perps',
                            'fx-major', 'fx-southeast-asia'));

-- A feed whose source publishes the inverse pair (e.g. JPY/CAD for canonical
-- CAD/JPY). Its observations are inverted at ingestion and say so.
ALTER TABLE quote_feeds
  ADD COLUMN inverted boolean NOT NULL DEFAULT false,
  -- Which elapsed time `stale_after_seconds` counts: every second, or
  -- weekdays only (UTC; rates published Monday to Friday). Policy only:
  -- the API's `ageMs` is always literal elapsed time.
  ADD COLUMN freshness_clock text NOT NULL DEFAULT 'continuous'
    CHECK (freshness_clock IN ('continuous', 'weekdays'));

ALTER TABLE market_observations
  ADD COLUMN inverted boolean NOT NULL DEFAULT false;

-- V1 EUR/USD is now the FX instrument EUR/USD (base EUR, quote USD) instead
-- of the currency EUR priced in USD. The curated V1 file declares Kraken's
-- ZEURZUSD feed for the new instrument; its old declaration (subject: the
-- EUR currency) is withdrawn so re-seeding does not conflict. Stored
-- observations are history and stay as they were normalized. Canonical
-- quotes of currency subjects are a derived cache and are dropped; nothing
-- declares a currency subject any more.
DELETE FROM quote_feeds
WHERE feed_source_id = 'kraken' AND symbol = 'ZEURZUSD' AND subject_category = 'currency';
DELETE FROM canonical_quotes WHERE subject_category = 'currency';
