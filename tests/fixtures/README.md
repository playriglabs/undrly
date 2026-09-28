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
| `sources/circle/`, `sources/solana/` | Rust provider, normalize and ingest tests (V1.5) | Circle's USDC contract address page (Markdown) and Solana `getGenesisHash` responses (mainnet, devnet), captured 2026-09-28, byte for byte. |
| `sources/robinhood-chain/`, `sources/rhj/` | Rust provider, normalize and ingest tests (V1.6) | Robinhood Chain `eth_chainId` (mainnet, testnet) and RHJ's asset registry (`/rhj/assets`, 195 assets), captured 2026-09-28. The Final Terms PDF is not stored here (3.2 MB); tests use synthetic bytes. |
| `sources/tempo/` | Rust provider and ingest tests (V1.7) | Tempo `eth_chainId` (mainnet 4217, Moderato 42431) and the pathUSD TIP-20 metadata batch from both, captured 2026-09-28. |
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

### V1.3 market data

Real responses captured on 2026-09-26 around 04:35Z, stored exactly as served.

| File | Request | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `kraken/ohlc-XXBTZUSD-60.json` | `GET https://api.kraken.com/0/public/OHLC?pair=XXBTZUSD&interval=60&since=1790377200` | 546 | `e310f501847c26a8e1ea8aad43ecee788d340f63343399404d9f4d14d19d95a1` |
| `hyperliquid/candles-BTC-1h.json` | `POST https://api.hyperliquid.xyz/info` candleSnapshot BTC 1h | 690 | `6bafe61f7aca618dd048dac521948e6409424893991c8fcb3f9b780e061de036` |
| `alpaca/bars-1Day.json` | `GET https://data.alpaca.markets/v2/stocks/bars?symbols=AAPL,NVDA&timeframe=1Day&start=2026-09-21T00:00:00Z&limit=10000&feed=iex&sort=asc` | 1,155 | `e4304dec53717759b87338a1f75c44b1491def868613e492450ab6792bb57c63` |
| `alpaca/calendar.json` | `GET https://paper-api.alpaca.markets/v2/calendar?start=2026-09-21&end=2026-12-31` | 9,289 | `fcf4bc02735b72f86a30536ef2beba650577fd418eebd095c1ee23b48fd2be90` |

### V1.3 event calendars

- `alpaca/corporate-actions.json`: real, `GET https://data.alpaca.markets/v1/corporate-actions?symbols=NVDA,AAPL,ADKT,ACLX,HURA&start=2025-09-01&end=2026-12-31&limit=1000`, captured 2026-09-26.
- `fred/release-dates-10.json`: real, FRED `release/dates?release_id=10` (CPI), 2026-08-01..2026-12-31 with future dates, captured 2026-09-26 (the API key is not in the file).
- `finnhub/earnings-calendar.json`: **synthetic**, in Finnhub's real format. Finnhub's free plan forbids sharing its data, so no capture is committed; `ZZZZ` is a made-up ticker and the NVDA figures are illustrative.

### V1.2 FX sources

Real responses captured on 2026-09-26 around 03:30Z (a Saturday; the
reference rates are Friday's, H.10's the previous week's), stored exactly as
served and marked binary in `.gitattributes`. Snapshots, not reference data.

| File | Request | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `bitstamp/ticker-eurusd.json` | `GET https://www.bitstamp.net/api/v2/ticker/eurusd/` | 271 | `24e3dea695ceb4bb28ba1aa8eb1f764a37b5b016d472ef67098f90730161fa94` |
| `ecb/eurofxref-daily.xml` | `GET https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml` | 1,547 | `2df818b9dabacdddb4408d24c00abd03fabab9dc2d9b6e261497450e4017bbce` |
| `bank-of-canada/observations.json` | `GET https://www.bankofcanada.ca/valet/observations/FXEURCAD,FXGBPCAD,FXAUDCAD,FXNZDCAD,FXJPYCAD,FXCHFCAD/json?recent=5` | 4,118 | `1f895d3e35f6d354ad3ec789d77072f5eda3a8cceec09f7028d7e0f843e36b5a` |
| `fed-h10/h10-lastobs10.csv` | `fed_h10::PACKAGE_URL` (H.10 package, last 10 observations) | 3,567 | `08b111163350ea97296c12a5822c82175a0d5f1651ef226a7e04cfd986a60681` |
| `bank-indonesia/jisdor-usd.xml` | `GET …/wskursbi.asmx/getSubKursJisdor3?mts=USD&startDate=2026-09-12&endDate=2026-09-26` | 6,039 | `68efbe22f9e31c3c2f8798b510b69aaa89808ac09ca90b7bf7cb02cd2fd93bbf` |
| `bank-indonesia/kurs-sgd.xml` | `GET …/wskursbi.asmx/getSubKursLokal3?mts=SGD&startdate=2026-09-12&enddate=2026-09-26` | 6,059 | `fe0bc69ef7c7291f6be764ec3f3a7d1816e2b030ebc8d597a9d7d7cb1c8245ba` |
| `bnm/exchange-rate-1700.json` | `GET https://api.bnm.gov.my/public/exchange-rate?session=1700&quote=rm` (`Accept: application/vnd.BNM.API.v1+json`) | 4,303 | `402d7c69b1f5d8ad819f0dd7999bd8463ee01670d66b27adccad519a726af8cf` |
| `cbm/latest.json` | `GET https://forex.cbm.gov.mm/api/latest` | 896 | `1507ffddccae97f247da08a5b901deb8bc33c3998cb098995fe693d755f05534` |

`alpaca/snapshots-NVDA.json` is a real IEX-feed snapshot captured
2026-09-25T07:11:19Z with the collector's request (Alpaca credentials as
headers only; none appear in the file):

| File | Request | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `alpaca/snapshots-NVDA.json` | `GET https://data.alpaca.markets/v2/stocks/snapshots?symbols=NVDA&feed=iex` | 606 | `2cf3fbfcd25b23d8853642127870d2dcb1f6e7b2dc4c6cb00ceab8a999377126` |

Its latest trade executed on IEX (`"x":"V"`) at `223.71`, at
`2026-09-24T20:45:15.183009877Z` (after the regular session: the capture ran
outside US market hours).
