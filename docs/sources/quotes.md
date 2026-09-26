# Quote sources (hackathon v1)

Five real upstreams (Kraken, Coinbase, Hyperliquid, gold-api, Alpaca) price
the demo universe, and a curated dataset supplies
reference data. Every response is stored raw (exact bytes) as a
`source_record` before anything is derived from it. Every observation names
that record.

> **Licensing status: unreviewed for every source.** Each is registered with
> `redistribution = unknown`, which Undrly treats as **restricted**. The data
> may be used **in local demo mode only** until a person reviews each
> provider's terms and records the result. Nothing here grants redistribution
> rights.

| Source id | Upstream | Auth | Feeds (symbol → subject in unit) | Basis / price type | Source time | Terms to review |
| --- | --- | --- | --- | --- | --- | --- |
| `kraken` | `GET https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD` | none | `XXBTZUSD` → Bitcoin in USD; `ZEURZUSD` → EUR in USD | venue (Kraken) / last + bid/ask | **none stated** (`observed_at = null`) | Kraken Terms of Service and API terms; market-data redistribution |
| `hyperliquid` | `POST https://api.hyperliquid.xyz/info {"type":"metaAndAssetCtxs"}` | none | `BTC` → BTC perpetual in **USDC** | venue (Hyperliquid) / mark | **none stated** | Hyperliquid terms of use; the response covers every perp (~72 KB), and only `BTC` is used |
| `gold-api` | `GET https://api.gold-api.com/price/XAU` | none | `XAU` → Gold (1 troy oz) in USD | aggregated / reference | `updatedAt` (seconds) | gold-api.com terms; the upstream contributors are undisclosed |
| `coinbase` | `GET https://api.exchange.coinbase.com/products/BTC-USD/book?level=1` | none | `BTC-USD` → Bitcoin in USD | venue (Coinbase Exchange) / mid + bid/ask | book `time` (ns, truncated to µs) | Coinbase Exchange API / market-data terms; Coinbase requires a User-Agent |
| `alpaca` | `GET https://data.alpaca.markets/v2/stocks/snapshots?symbols=NVDA&feed=iex` | `APCA-API-KEY-ID` / `APCA-API-SECRET-KEY` headers | `NVDA` → NVIDIA common stock in USD | **venue (IEX)** / last (trade) + mid (IEX top of book, with bid/ask) | trade time `t` / quote time `t` (ns, truncated to µs) | Alpaca Market Data agreement; the IEX feed's display and redistribution terms |
| `undrly-curated` | `data/demo/universe.json` (this repository) | — | declares all feeds above | — | — | Undrly-authored; identifiers in it (ISIN, FIGI, LEI, MIC) were checked against their registries |

## Semantics that must not be blurred

- **NVDA is an IEX venue quote**, delivered by Alpaca: `basis = venue`,
  `venue = IEX` (the observation's source is `alpaca`; the canonical quote
  names only the venue). Two feeds per symbol, each its own observation
  with its own time: the **last** trade executed on IEX, and the **mid** of
  IEX's own top of book with that bid and ask. The book is IEX's alone, not
  a consolidated (NBBO) quote. Only a book **quoted during the regular US
  session** (09:30–16:00 New York time, Monday–Friday, by the quote's own
  time; US DST rule; no holiday table, as IEX stamps no quotes when closed;
  early-close days count as full days) yields a mid observation: outside it
  IEX's book is a few resting orders (e.g. NVDA 225.05 / 230.53 after the
  close), not a market. A side with no IEX interest (price `0`, blank
  exchange) or a crossed book yields none either. The canonical quote is the newer
  of the two (`latest-observation-v1`), so `priceType` is `last` or `mid`.
  It is always presented as an IEX venue quote, never as a price for other
  venues. The normalizer rejects any trade or quote side whose exchange
  code is not `V` (IEX).
- **The BTC perpetual is priced in USDC**, its settlement asset. It is not
  converted to USD.
- **Gold is aggregated**: gold-api does not name a venue, so no venue is
  claimed. A response in a currency other than USD is rejected rather than
  mislabelled.
- **BTC/USD has two venue feeds** (Kraken, Coinbase). Each is stored as its
  own venue observation. The canonical BTC/USD quote is `mean-venue-mid-v1`
  (see `docs/hackathon-v1.md` §13): `basis = aggregated`, attributed to no
  venue or source, with the mean bid and mean ask of its inputs. Coinbase's observation is its level-1 mid (normalizer
  computes `(bid + ask) / 2` exactly), with the book time as source time.
- **Kraken and Hyperliquid state no timestamp.** Their observations have
  `observed_at = null`, and freshness uses `received_at`. Undrly's clock is
  never presented as source time.

## Request behaviour

`undrly-collect run` polls **sequentially**, one request at a time, at
fixed intervals: Kraken 10 s, Coinbase 10 s, Hyperliquid 15 s, gold-api
60 s, Alpaca 15 s.
There are no retries beyond the next tick, no concurrency, no redirects, a
30 s timeout and a 32 MiB response limit.

- The User-Agent is `Undrly/0.1 (+https://undrly.xyz)`. Override it with
  `UNDRLY_USER_AGENT`.
- Alpaca credentials come only from `APCA_API_KEY_ID` and
  `APCA_API_SECRET_KEY`. They are sent as headers and never stored, logged,
  or placed in a record key.

## Replay and storage

- A response identical to a stored one (same source, record key and
  SHA-256) is the same raw record, so a replay changes nothing.
- A new response restating a price the source already timestamped (gold-api
  re-serving the same `updatedAt`) is a new raw record but the **same**
  observation.
- Kraken and Hyperliquid responses have no source time, so each distinct
  response is its own observation.
- There is no raw-record retention policy yet. Hyperliquid's ~72 KB
  response every 15 s is about 17 MB/hour locally. That is acceptable for a
  demo, but it has to be addressed before long-running use.
