-- Bid and ask on canonical quotes.
--
-- `mean-venue-mid-v1` now also states the mean bid and the mean ask of the
-- very inputs whose mids it averages (not a best bid/offer: no max/min is
-- taken). They are part of the aggregation's output, so they are stored
-- beside its price; each is reproducible from the inputs' observations.
-- `latest-observation-v1` carries its single observation's bid and ask, if
-- any.
--
-- canonical_quotes is a derived cache that the collector rebuilds. Existing
-- mean-venue-mid-v1 rows have no bid/ask to backfill without recomputing, so
-- they are removed (their inputs cascade) and the next refresh writes them
-- again. latest-observation-v1 rows stay; their bid/ask is filled from their
-- one input observation.
DELETE FROM canonical_quotes WHERE method = 'mean-venue-mid-v1';

ALTER TABLE canonical_quotes
  ADD COLUMN bid financial_decimal,
  ADD COLUMN ask financial_decimal;

UPDATE canonical_quotes c
SET bid = o.bid, ask = o.ask
FROM canonical_quote_inputs i
JOIN market_observations o ON o.id = i.observation_id
WHERE c.method = 'latest-observation-v1'
  AND i.subject_id = c.subject_id AND i.unit_id = c.unit_id;

ALTER TABLE canonical_quotes
  ADD CONSTRAINT canonical_quotes_bid_ask
    CHECK ((bid IS NULL) = (ask IS NULL) AND (bid IS NULL OR bid <= ask)),
  -- A mean of venue mids always has a mean bid and ask around its price.
  ADD CONSTRAINT canonical_quotes_mean_has_bid_ask CHECK (
    method <> 'mean-venue-mid-v1' OR (bid IS NOT NULL AND bid <= price AND price <= ask)
  );
