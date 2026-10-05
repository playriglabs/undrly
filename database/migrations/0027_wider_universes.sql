-- V1.10 (docs/v1.10-tokenized-stocks.md §11): wider crypto and equity
-- universes. Additive: no existing row changes.
ALTER TABLE universe_snapshots
  DROP CONSTRAINT universe_snapshots_universe_key_check,
  ADD CONSTRAINT universe_snapshots_universe_key_check
    CHECK (universe_key IN ('crypto-top100', 'crypto-top250', 'crypto-top500', 'sp500', 'sp400',
                            'sp600', 'nasdaq100', 'hyperliquid-perps',
                            'fx-major', 'fx-southeast-asia', 'fx-global'));
