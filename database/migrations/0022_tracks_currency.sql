-- V1.7 stablecoin reference currency (docs/v1.7-tempo.md §12).
--
-- instrument TRACKS currency: the instrument is designed to be worth, and so
-- to follow, one unit of the currency (e.g. a TIP-20 stablecoin's immutable
-- `currency()` declaration, "the reference asset that 1 unit of the token is
-- designed to be worth"). It is not identity, not a claim, not redemption or
-- backing, and not a denomination.
INSERT INTO relationship_rules (relationship_type, subject_category, object_category) VALUES
  ('TRACKS', 'instrument', 'currency');
