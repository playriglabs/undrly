-- V1.10 (docs/v1.10-tokenized-stocks.md): tokenized stocks and wider
-- universes. Additive: no existing row changes.

-- CoinGecko's top 250 (the top 100 stays, as its first 100 ranks).
ALTER TABLE universe_snapshots
  DROP CONSTRAINT universe_snapshots_universe_key_check,
  ADD CONSTRAINT universe_snapshots_universe_key_check
    CHECK (universe_key IN ('crypto-top100', 'crypto-top250', 'sp500', 'nasdaq100',
                            'hyperliquid-perps', 'fx-major', 'fx-southeast-asia', 'fx-global'));

-- A record key is the request that produced the record. A Jupiter price
-- request names up to 40 Solana mints (~1,850 characters); the record key
-- keeps the whole request instead of a shortened stand-in. 2,048 keeps the
-- replay key's btree entries (source, key, SHA-256) under PostgreSQL's
-- ~2,700-byte index row limit.
ALTER TABLE source_records
  DROP CONSTRAINT source_records_record_key_check,
  ADD CONSTRAINT source_records_record_key_check
    CHECK (record_key <> '' AND char_length(record_key) <= 2048 AND record_key !~ '[[:cntrl:]]');
