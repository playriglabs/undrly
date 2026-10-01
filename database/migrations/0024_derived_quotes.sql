-- Live FX through stablecoin venues (docs/v1.9-live-fx.md).
--
-- `cross-via-stablecoin-v1`: a pair's canonical quote is the ratio of two
-- other pairs' canonical quotes (USD/IDR = USDT/IDR / USDT/USD), never of its
-- own observations (undrly_core::quote::cross_quote). It is declared per pair
-- in `quote_derivations`, which names both legs; its inputs are the legs'
-- observations, kept in `canonical_quote_legs` (`canonical_quote_inputs`
-- only admits observations of the very pair it aggregates).
ALTER TABLE quote_aggregations
  DROP CONSTRAINT quote_aggregations_method_check,
  ADD CONSTRAINT quote_aggregations_method_check
    CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1', 'mark-with-venue-book-v1'));

ALTER TABLE canonical_quotes
  DROP CONSTRAINT canonical_quotes_method_check,
  ADD CONSTRAINT canonical_quotes_method_check
    CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1', 'mark-with-venue-book-v1',
                      'cross-via-stablecoin-v1')),
  -- A cross is a derived mid, never a venue's or an aggregate's price.
  ADD CONSTRAINT canonical_quotes_cross_is_derived CHECK (
    method <> 'cross-via-stablecoin-v1' OR (basis = 'derived' AND price_type = 'mid')
  );

-- price(subject in unit) = price(numerator) / price(denominator).
CREATE TABLE quote_derivations (
  subject_id                 uuid NOT NULL,
  subject_category           text NOT NULL CHECK (subject_category IN ('instrument', 'currency')),
  unit_id                    uuid NOT NULL,
  unit_category              text NOT NULL CHECK (unit_category IN ('currency', 'instrument')),
  method                     text NOT NULL CHECK (method = 'cross-via-stablecoin-v1'),
  numerator_subject_id       uuid NOT NULL,
  numerator_subject_category text NOT NULL,
  numerator_unit_id          uuid NOT NULL,
  numerator_unit_category    text NOT NULL,
  denominator_subject_id     uuid NOT NULL,
  denominator_subject_category text NOT NULL,
  denominator_unit_id        uuid NOT NULL,
  denominator_unit_category  text NOT NULL,
  source_id                  source_id NOT NULL,
  received_at                timestamptz NOT NULL,
  source_record_id           bigint NOT NULL,
  PRIMARY KEY (subject_id, unit_id),
  FOREIGN KEY (subject_id, subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (unit_id, unit_category) REFERENCES nodes (id, category),
  FOREIGN KEY (numerator_subject_id, numerator_subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (numerator_unit_id, numerator_unit_category) REFERENCES nodes (id, category),
  FOREIGN KEY (denominator_subject_id, denominator_subject_category) REFERENCES nodes (id, category),
  FOREIGN KEY (denominator_unit_id, denominator_unit_category) REFERENCES nodes (id, category),
  CONSTRAINT quote_derivations_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  -- A cross shares its stablecoin between legs, and prices neither leg itself.
  CONSTRAINT quote_derivations_shared_via CHECK (numerator_subject_id = denominator_subject_id),
  CONSTRAINT quote_derivations_numerator_in_unit CHECK (numerator_unit_id = unit_id),
  CONSTRAINT quote_derivations_not_a_leg CHECK (
    subject_id <> numerator_subject_id AND unit_id <> denominator_unit_id
  )
);
CREATE INDEX quote_derivations_numerator_idx
  ON quote_derivations (numerator_subject_id, numerator_unit_id);
CREATE INDEX quote_derivations_denominator_idx
  ON quote_derivations (denominator_subject_id, denominator_unit_id);
CREATE INDEX quote_derivations_source_record_idx ON quote_derivations (source_record_id);

-- The observations a derived canonical quote was computed from: each leg's
-- own inputs, priced in that leg's pair.
CREATE TABLE canonical_quote_legs (
  subject_id      uuid NOT NULL,
  unit_id         uuid NOT NULL,
  observation_id  bigint NOT NULL,
  leg_subject_id  uuid NOT NULL,
  leg_unit_id     uuid NOT NULL,
  input_price     financial_decimal NOT NULL,
  PRIMARY KEY (subject_id, unit_id, observation_id),
  FOREIGN KEY (subject_id, unit_id) REFERENCES canonical_quotes (subject_id, unit_id)
    ON DELETE CASCADE,
  FOREIGN KEY (observation_id, leg_subject_id, leg_unit_id)
    REFERENCES market_observations (id, subject_id, unit_id)
);
CREATE INDEX canonical_quote_legs_observation_idx ON canonical_quote_legs (observation_id);

-- FX pairs outside the G10 majors and Southeast Asia, priced live through
-- stablecoin venues (USD/HKD, USD/AED, USD/BRL, USD/MXN).
ALTER TABLE universe_snapshots
  DROP CONSTRAINT universe_snapshots_universe_key_check,
  ADD CONSTRAINT universe_snapshots_universe_key_check
    CHECK (universe_key IN ('crypto-top100', 'sp500', 'nasdaq100', 'hyperliquid-perps',
                            'fx-major', 'fx-southeast-asia', 'fx-global'));
