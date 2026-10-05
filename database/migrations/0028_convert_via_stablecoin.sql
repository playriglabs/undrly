-- V1.10 (docs/v1.10-tokenized-stocks.md §12): `convert-via-stablecoin-v1`.
-- A market quoted only in a stablecoin restated in the currency it tracks:
-- price(X in USD) = price(X in USDT) × price(USDT in USD)
-- (undrly_core::quote::convert_quote). Declared in `quote_derivations` like
-- a cross; for a conversion the numerator is the subject in the stablecoin
-- and the denominator the stablecoin in the unit. Additive.
ALTER TABLE canonical_quotes
  DROP CONSTRAINT canonical_quotes_method_check,
  ADD CONSTRAINT canonical_quotes_method_check
    CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1', 'mark-with-venue-book-v1',
                      'cross-via-stablecoin-v1', 'convert-via-stablecoin-v1')),
  DROP CONSTRAINT canonical_quotes_cross_is_derived,
  ADD CONSTRAINT canonical_quotes_cross_is_derived CHECK (
    method NOT IN ('cross-via-stablecoin-v1', 'convert-via-stablecoin-v1')
    OR (basis = 'derived' AND price_type = 'mid')
  );

ALTER TABLE quote_derivations
  DROP CONSTRAINT quote_derivations_method_check,
  ADD CONSTRAINT quote_derivations_method_check
    CHECK (method IN ('cross-via-stablecoin-v1', 'convert-via-stablecoin-v1')),
  DROP CONSTRAINT quote_derivations_shared_via,
  DROP CONSTRAINT quote_derivations_numerator_in_unit,
  DROP CONSTRAINT quote_derivations_not_a_leg,
  -- A cross shares its stablecoin between legs, and prices neither leg itself.
  ADD CONSTRAINT quote_derivations_cross_legs CHECK (
    method <> 'cross-via-stablecoin-v1' OR (
      numerator_subject_id = denominator_subject_id
      AND numerator_unit_id = unit_id
      AND subject_id <> numerator_subject_id
      AND unit_id <> denominator_unit_id
    )
  ),
  -- A conversion chains its legs: subject in stablecoin, stablecoin in unit.
  ADD CONSTRAINT quote_derivations_convert_legs CHECK (
    method <> 'convert-via-stablecoin-v1' OR (
      numerator_subject_id = subject_id
      AND numerator_unit_id = denominator_subject_id
      AND denominator_unit_id = unit_id
      AND subject_id <> denominator_subject_id
    )
  );
