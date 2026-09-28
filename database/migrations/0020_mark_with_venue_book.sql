-- Perpetual quotes with the venue's order book (docs/v1.5-solana.md, §16).
--
-- `mark-with-venue-book-v1`: a derivative's canonical quote is its venue mark
-- price; its bid and ask are the same venue's best book levels when the book
-- is within 60 s of the mark (undrly_core::quote). Declared per pair in
-- `quote_aggregations`; the book is its own feed (price type `mid`).
ALTER TABLE quote_aggregations
  DROP CONSTRAINT quote_aggregations_method_check,
  ADD CONSTRAINT quote_aggregations_method_check
    CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1', 'mark-with-venue-book-v1'));

ALTER TABLE canonical_quotes
  DROP CONSTRAINT canonical_quotes_method_check,
  ADD CONSTRAINT canonical_quotes_method_check
    CHECK (method IN ('latest-observation-v1', 'mean-venue-mid-v1', 'mark-with-venue-book-v1')),
  -- The quote is the venue's mark: never aggregated, never another price type.
  ADD CONSTRAINT canonical_quotes_mark_is_venue CHECK (
    method <> 'mark-with-venue-book-v1' OR (basis = 'venue' AND price_type = 'mark')
  );
