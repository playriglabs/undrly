-- V1.6 tracker certificates (docs/v1.6-robinhood-chain.md §6).
--
-- instrument TRACKS instrument: the subject's value tracks the object's
-- market value by its terms, without a legal or beneficial claim on it (e.g.
-- a collateralised tracker certificate whose Final Terms say it "tracks the
-- market value of the Underlying"). Not TOKENIZES (a claim on the object) and
-- not DERIVES_FROM (a derivative contract).
INSERT INTO relationship_rules (relationship_type, subject_category, object_category) VALUES
  ('TRACKS', 'instrument', 'instrument');
