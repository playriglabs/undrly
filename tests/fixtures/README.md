# Fixtures

Deterministic test data. Values (prices, symbols, relationships) are
illustrative test data, not market or reference data, and must never be loaded
into a database outside tests. Public identifiers in `identifiers.json` (for
example Apple's ISIN and LEI) are used only as check-digit test vectors.

| Path | Used by | Purpose |
| --- | --- | --- |
| `shared/primitives.json` | Rust core, TypeScript | Canonical ID text ↔ UUID pairs, decimals, timestamps, source ids. Both languages must agree on every case. |
| `shared/vocabulary.json` | Rust core, Rust store (vs. database), TypeScript | Categories, classes, relationship types and rules, observation bases, identifier namespaces. Every side asserts its tables equal this file. |
| `identifiers.json` | Rust core, Rust store (vs. database) | Namespace-specific identifier cases. The database checks shape only, so every Rust-valid value must pass the database checks. |
| `sources/reference-fixture/` | Rust ingest tests | Raw records for the deterministic fixture provider (see below). |
| `sources/sec-edgar/` | Rust provider and ingest tests | Captured SEC EDGAR responses, byte for byte (see below). |
| `sources/kraken/`, `sources/coinbase/`, `sources/hyperliquid/`, `sources/gold-api/`, `sources/alpaca/` | Rust provider, normalize and ingest tests | Captured quote responses, byte for byte (see below). |
| `api/v1/` | TypeScript | External JSON API contract v1: valid documents (accepted unchanged) and `invalid/` (rejected). |

Released API fixtures are append-only; a breaking change goes in `api/v2/`.

## `sources/reference-fixture/`

Raw payloads in Undrly's own fixture format (not any provider's API),
decoded by `undrly_provider::fixture`.

- `nvda.json`: NVIDIA / NVDA. Every identifier was checked against its
  registry on 2026-09-24:
  - LEI `549300S4KLFTLO7GSQ80`: GLEIF, legal name "NVIDIA CORPORATION",
    status ACTIVE.
  - ISIN `US67066G1040`: OpenFIGI mapping to NVIDIA CORP common stock.
  - Share-class FIGI `BBG001S5TZJ6` and Nasdaq exchange-level FIGI
    `BBG000BBK0R0` (exchange code `UW`): OpenFIGI.
  - MIC `XNAS` (ISO 10383) and `USD` (ISO 4217).

  The US composite FIGI (`BBG000BBJQV0`) is left out on purpose: it
  identifies neither the global security nor a single listing, and the
  schema has no category for it. The record gives no validity bounds, so
  every claim is unbounded ("bounds unknown").
- `synthetic-conflict.json`: **synthetic**. A made-up company whose ISIN
  (`ZZ…`) and LEI (`SYNTHETIC…`) have valid check digits but are not
  registered. It claims NVDA's Nasdaq symbol and exchange FIGI, and exists
  only to test conflict quarantine.

## `sources/sec-edgar/`

Real SEC EDGAR responses, stored exactly as served (no trailing newline, no
reformatting; `.gitattributes` marks them binary so no tool rewrites them).

- `CIK0001045810.json`: `GET https://data.sec.gov/submissions/CIK0001045810.json`
  (NVIDIA CORP), captured 2026-09-25T02:14:18Z, 159,785 bytes, SHA-256
  `e220b9939d680e34f3a59655b5134a8f1a44e6a4d9292426093c8a899d3020a8`. SEC
  EDGAR data is public; the file is a snapshot and goes stale as NVIDIA
  files. Tests derive malformed variants from it in code.

## Quote sources (`kraken/`, `hyperliquid/`, `gold-api/`, `alpaca/`)

Real responses captured on 2026-09-25 around 03:52Z with a generic Undrly
User-Agent and no compression. They are stored exactly as served and marked
binary in `.gitattributes`. Prices are snapshots, not reference data.

| File | Request | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `kraken/ticker.json` | `GET https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD` | 571 | `a8a7a77d46fe2d61816bfa00ef808a9f2ee15be5db6b5341b6ee61eb45cbcc57` |
| `hyperliquid/metaAndAssetCtxs.json` | `POST https://api.hyperliquid.xyz/info {"type":"metaAndAssetCtxs"}` | 72,414 | `90703344db921f6ec543a05859c03f2acc17832aff85e08ff6c8530a39ac4232` |
| `gold-api/price-XAU.json` | `GET https://api.gold-api.com/price/XAU` | 182 | `abe307bb996b84fcbe4538ff4d9663b9689000088102079c5ebb33d06d5b19e0` |
| `coinbase/book-BTC-USD-level1.json` | `GET https://api.exchange.coinbase.com/products/BTC-USD/book?level=1` (captured 2026-09-25T07:28:07Z) | 175 | `140a42cd1ef89975fb57649c392da89dfa2c3457f25a10e1ea9870ef7f8efd42` |

`alpaca/snapshots-NVDA.json` is a real IEX-feed snapshot captured
2026-09-25T07:11:19Z with the collector's request (Alpaca credentials as
headers only; none appear in the file):

| File | Request | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `alpaca/snapshots-NVDA.json` | `GET https://data.alpaca.markets/v2/stocks/snapshots?symbols=NVDA&feed=iex` | 606 | `2cf3fbfcd25b23d8853642127870d2dcb1f6e7b2dc4c6cb00ceab8a999377126` |

Its latest trade executed on IEX (`"x":"V"`) at `223.71`, at
`2026-09-24T20:45:15.183009877Z` (after the regular session: the capture ran
outside US market hours).
