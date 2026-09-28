-- V1.4.1 Hyperliquid contract semantics
-- (docs/v1.4-cross-ecosystem-identity.md, "Hyperliquid contract semantics").
--
-- Hyperliquid's contract specification: "USDC margining, USDT denominated
-- linear contracts. That is, the oracle price is denominated in USDT, but the
-- collateral is USDC." (PURR and HYPE are the documented USDC-denominated
-- exceptions.) Undrly declared every perpetual's mark feed in USDC and had no
-- way to say what the collateral is.

-- instrument → the currency or asset posted as its margin (collateral).
-- Separate from SETTLES_IN (what its cash flows are paid in) and
-- DENOMINATED_IN (its price unit), which may all differ.
INSERT INTO relationship_rules (relationship_type, subject_category, object_category) VALUES
  ('MARGINED_IN', 'instrument', 'currency'),
  ('MARGINED_IN', 'instrument', 'instrument');

-- Withdraw every Hyperliquid feed declaration. `undrly-collect seed`, which
-- applies this migration, re-declares each perpetual's mark feed with its
-- documented price unit (Tether USD for USDT-denominated perpetuals, USD
-- Coin for PURR and HYPE). A feed declaration is a current assertion, not
-- history; its unit cannot change in place (one declaration per source,
-- symbol and price type).
DELETE FROM quote_feeds WHERE feed_source_id = 'hyperliquid';

-- Canonical quotes are a derived cache: drop those of perpetuals so none is
-- served under the withdrawn unit. The next collection pass recomputes them
-- from observations normalized under the corrected declarations.
DELETE FROM canonical_quote_inputs
WHERE subject_id IN (SELECT id FROM instruments WHERE instrument_class = 'perpetual_future');
DELETE FROM canonical_quotes
WHERE subject_id IN (SELECT id FROM instruments WHERE instrument_class = 'perpetual_future');

-- Deliberately unchanged: raw records, and the observations, perpetual
-- contexts and bars already normalized with unit USD Coin. They stay as they
-- were normalized (historical rows are never relabelled); the API serves a
-- perpetual only under its declared unit, so they are no longer served.
