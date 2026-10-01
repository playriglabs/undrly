# Undrly

**One normalized API across every market.**

Collect fragmented market data, normalize it, aggregate it, and serve it
through one interface.

A stock on an exchange, a crypto pair on two venues, an FX rate, a gold
price and a perpetual future come from different providers, in different
formats, with different meanings. Undrly turns them into one consistent quote
you can ask for with one request:

```bash
curl -s localhost:8787/v1/quote/BTC/USD | jq
```

## Markets today (hackathon v1)

| Market | Query | Source(s) | What `/v1/quote` returns |
| --- | --- | --- | --- |
| Equity | `NVDA` | Alpaca (IEX feed) | the newer of IEX's last trade and IEX's book mid (with bid/ask, regular session only): an IEX venue quote |
| Crypto spot | `BTC/USD` | Kraken + Coinbase | **one aggregate** of both venues' mid prices |
| FX | `EUR/USD` | Kraken + Bitstamp | **one aggregate** of both venues' mid prices (V1.2; V1 served Kraken alone) |
| Commodity | `XAU/USD` | gold-api | an aggregated reference price per troy ounce |
| Perpetual | `BTC-PERP` | Hyperliquid | the mark price, in USDT (its denomination; margined in USDC) |

Every answer has the same shape, whatever the market. This set is frozen for
the hackathon (see [Scope](#scope)).

## Universe expansion (V1.1, local)

V1.1 widens this to real universes, without changing the quote model
([`docs/v1.1-universe.md`](docs/v1.1-universe.md)):

- **crypto top 100** (CoinGecko): priced by Kraken and Coinbase USD markets,
  mapped through CoinGecko's exchange tickers, never by symbol;
- **S&P 500**: via SPY holdings, which is a proxy, not the official file;
- **Nasdaq-100**: universe support is modeled, but live membership import is
  deferred pending an approved machine-readable source;
- **every live Hyperliquid perpetual**, with contract multipliers (`kPEPE` = 1,000 PEPE);
- **14 commodities**: gold-api metals, EIA oil and gas, and World Bank monthly
  averages.

## FX universes (V1.2, local)

V1.2 adds two curated FX universes ([`docs/v1.2-fx.md`](docs/v1.2-fx.md)):

- **`fx-major`** (29 pairs): live venue books where they are liquid
  (Kraken; Kraken + Bitstamp for EUR/USD and GBP/USD), otherwise
  central-bank **reference** rates (ECB, Bank of Canada, Federal Reserve
  H.10). 10 crosses have no approved direct source and return `no_quote`.
- **`fx-southeast-asia`** (10 pairs): official reference rates (Bank
  Indonesia JISDOR and transaction rates, Bank Negara Malaysia, Central Bank
  of Myanmar, H.10, ECB). USD/VND has no approved machine-readable source;
  since V1.9, USD/IDR, USD/THB, USD/SGD and USD/PHP are priced live as
  derived crosses (below).

An FX market is an instrument of class `fx` between two currency nodes
(`EUR/USD`: 1 EUR in USD). Query it as `EUR/USD`, `eur/usd` or `EURUSD`;
`USD/EUR` is a different market and is not served inverted. Reference rates
carry `priceType: reference` and no bid/ask, and are never aggregated with
venue quotes. The FX definition is Undrly-authored and committed
(`data/reference/fx-spec.json` → `undrly-collect fx build` →
`data/reference/fx.json`).

Universe data is third-party and stays **local** (`data/universe/` is
git-ignored). Build it once per machine:

```bash
# .env: UNDRLY_SEC_USER_AGENT="Your Name you@example.com"; optional EIA_API_KEY
(cd rust && cargo build -q -p undrly-collect)
(set -a; . ./.env; set +a; ./rust/target/debug/undrly-collect universe fetch)
./rust/target/debug/undrly-collect universe build   # snapshot + data/universe/report.md
./scripts/dev.sh                                     # seeds the snapshot if present
curl -s localhost:8787/v1/universes | jq
```

Symbols can collide across asset classes. The unambiguous forms are a pair
(`ETH/USD`), a venue symbol (`NASDAQ:AAPL`, `NYSE:KO` or `XNYS:KO`,
`COINBASE:ETH-USD`, `HYPERLIQUID:ETH`), an identifier, or an id. Class
shares take either punctuation (`BRK.B` or `BRK-B`). Equities have no constructed
ISIN: identity is the Undrly id, and issuers carry their SEC CIK.

## Market data (V1.3, local)

Candles, reference history, market context and perpetual data for the same
instruments, through the same query language
([`docs/v1.3-market-data.md`](docs/v1.3-market-data.md)):

```bash
./scripts/history.sh      # backfill bars, reference series, calendar (~25 min), beside dev.sh
curl -s "localhost:8787/v1/candles/BTC/USD?interval=1h&limit=5" | jq
curl -s localhost:8787/v1/market/AAPL | jq          # marketStatus + session statistics
curl -s localhost:8787/v1/derivatives/BTC-PERP | jq  # mark, index, funding, open interest
curl -s "localhost:8787/v1/history/USD/IDR?limit=5" | jq
curl -s "localhost:8787/v1/calendar/AAPL?from=2026-11-23&to=2026-11-30" | jq   # Thanksgiving, early close
curl -s "localhost:8787/v1/economic-calendar?category=inflation" | jq
```

Candles are one venue's trade bars (Kraken, Hyperliquid, IEX; not named in the response);
`4h` is derived from four complete hours, never filled. Reference rates have
history, never OHLC. Missing data is `null` or `404 no_data`.

## Agents / MCP (V1.8, local)

Undrly as an MCP server (stdio) for AI agents: nine read-only tools over the
same resolver, explain, graph and market-data code as the REST API
([`docs/v1.8-mcp.md`](docs/v1.8-mcp.md)). It provides stored facts with their
units and provenance; it never infers, ranks or writes.

```bash
UNDRLY_DB=undrly_v14 ./scripts/local-db.sh mcp.sh    # verify the server end to end (both protocol eras)
```

```json
{ "mcpServers": { "undrly": {
    "command": "bun",
    "args": ["run", "<path to undrly>/typescript/apps/mcp/src/stdio.ts"],
    "env": { "DATABASE_URL": "postgres://undrly:<password>@127.0.0.1:55432/undrly_v14" } } } }
```

Tools: `search_instruments`, `resolve_instrument`, `explain_instrument`,
`get_instrument`, `get_instrument_graph`, `get_quote`, `get_markets`,
`get_history`, `get_derivatives`; resource `undrly://vocabulary`.

## Live FX through stablecoins (V1.9, local)

V1.9 ([`docs/v1.9-live-fx.md`](docs/v1.9-live-fx.md)) prices FX live where
V1.2 had only daily reference rates:

- **Stablecoin/fiat markets** from venue books: USDT/USDC in IDR (Binance,
  Indodax), THB (Bitkub), PHP (Coins.ph), SGD (OKX, Coinbase), HKD (HashKey),
  AED, BRL, MXN (Binance, OKX, Coinbase), and EUR, GBP, AUD, CAD (Kraken,
  Coinbase, OKX); EURC in USD and EUR. Query them like any pair:
  `/v1/quote/USDT/IDR`.
- **Derived FX**: `USD/IDR`, `USD/THB`, `USD/PHP`, `USD/SGD`, `USD/HKD`,
  `USD/AED`, `USD/BRL`, `USD/MXN` = `USDT/X ÷ USDT/USD`, served with
  `basis: derived` and method `cross-via-stablecoin-v1`, 24h statistics and
  hourly history from the legs' closes.
- Books wider than 10 bps at research are excluded (USD/JPY stays a
  reference rate); CNY and SAR have no source and are listed as unsupported.

The definition is committed (`data/reference/stablecoin-fx-spec.json` →
`undrly-collect fx build` → `data/reference/stablecoin-fx.json`).

## Quickstart

Needs Docker, Rust (`rustup`), Bun ≥ 1.4 and `jq`.

```bash
cp .env.example .env    # set UNDRLY_POSTGRES_PASSWORD; add Alpaca keys for NVDA
./scripts/dev.sh        # PostgreSQL + collector + API on http://127.0.0.1:8787
```

The first run compiles the Rust collector (a few minutes); later runs start
in seconds. Then, in another terminal:

```bash
curl -s localhost:8787/ | jq                     # what's here
curl -s localhost:8787/v1/quote/BTC/USD | jq     # one canonical BTC/USD quote
curl -s localhost:8787/v1/quotes/BTC/USD | jq    # the Kraken and Coinbase observations behind it
curl -s localhost:8787/v1/quote/NVDA | jq        # same shape, a different market
curl -s "localhost:8787/v1/search?q=gold" | jq   # find things
```

For a guided terminal walkthrough: `./scripts/present.sh --step`.

## API

All routes are `GET`, read-only, and answer from Undrly's own storage.

| Route | Returns |
| --- | --- |
| `/` | service name, endpoints, example queries |
| `/v1/quote/{query}` | **one canonical quote** for a market |
| `/v1/quotes/{query}` | the per-source observations behind it |
| `/v1/search?q=` | matching instruments, currencies and venues |
| `/v1/resolve?q=` | what a query refers to: `resolved`, `ambiguous` or `not_found` |
| `/v1/explain?q=` | why a query resolves the way it does: the matching rule and stored value per candidate, identifiers, relationships (V1.4) |
| `/v1/instruments/{id}/graph` | an instrument's direct relationships and listings, plus its chain deployments and priced markets (V1.4) |
| `/v1/universes`, `/v1/universes/{key}` | imported universes and their latest membership (V1.1) |
| `/v1/candles/{query}?interval=1h\|4h\|1d&limit=` | a market's venue candles (OHLCV), oldest first (V1.3) |
| `/v1/history/{query}?limit=` | a reference series' published values (central-bank rates, commodity references) |
| `/v1/market/{query}` | the quote with market status (`continuous`, `open`, `closed`, …) and session or rolling-24h statistics |
| `/v1/markets?class=&q=&limit=&offset=` | every quoted market, paged and filterable by class and name, each with market status, statistics and a 24h sparkline |
| `/v1/derivatives/{query}` | a perpetual's mark, index, funding rate and open interest |
| `/v1/calendar/{query}?from=&to=` | a stock's trading days, hours, early closes, holidays, corporate actions (dividends, splits, mergers…) and earnings dates with estimates |
| `/v1/economic-calendar?from=&to=&category=` | scheduled US economic releases (CPI, jobs report, GDP, PCE, …) |

- **Queries:** a symbol or name (`NVDA`, `Gold`, `BTC perpetual`), a pair
  (`EUR/USD`), an identifier (`isin:US67066G1040`), a venue symbol
  (`NASDAQ:NVDA`), or an Undrly id.
- **Prices** are exact decimal strings (`"83839.5975000"`), never floats. The
  trailing digits are intentional.
- **Every quote** says what it is: `basis` (`venue` or `aggregated`), the
  `venue` for a venue quote, the `unit` (a currency such as USD, or an asset
  such as USDC), `asOf` and `freshness`. It does not name the data provider;
  `/v1/quotes` does. NVDA shows `stale` outside US market hours; it is the
  newer of the last IEX trade and IEX's book mid.
- **Errors** are JSON: `bad_request` (400), `not_found` / `no_quote` (404),
  `ambiguous` (409).
- **One market by id** (V1.8): the quote, quotes, candles, history, market,
  derivatives and calendar routes take `?unit=<currency or asset id>` to pick
  one of an instrument's markets.

Full contract: [`docs/contracts.md`](docs/contracts.md).

## How it works

```text
collect     providers fetch each source; the exact response is stored first
normalize   each response becomes observations in one model (price, unit, venue, time)
aggregate   observations become one canonical quote per market
serve       the API reads canonical quotes from storage
```

- The **collector** (Rust) polls sources one at a time in the background.
- **BTC/USD** averages the mid prices of fresh Kraken and Coinbase quotes
  (`mean-venue-mid-v1`, 30 s freshness window); its `bid`/`ask` are the mean
  bid and mean ask of the same quotes (not a best bid/offer). The venue observations stay
  available at `/v1/quotes/BTC/USD`.
- The **API** (TypeScript) never calls a provider while answering a request,
  so its speed doesn't depend on any upstream.
- Every quote traces back to the exact upstream response it came from.

## Data sources and licensing

Upstream data is used in **local/private demo mode only**. The
redistribution terms of every source (Kraken, Coinbase, Hyperliquid,
gold-api, Alpaca/IEX; for V1.1 also CoinGecko, SSGA, Nasdaq, SEC, EIA and
the World Bank; for V1.2 also Bitstamp, the ECB, the Bank of Canada, the
Federal Reserve, Bank Indonesia, Bank Negara Malaysia and the Central Bank of
Myanmar; for V1.3 also FRED and Finnhub, whose free plan is personal-use
only; for V1.9 also Binance, OKX, Indodax, Bitkub, Coins.ph and HashKey) are **unreviewed**, and Undrly does **not** currently
claim production redistribution rights for any of them. Do not expose this
data publicly. Details per source: [`docs/sources/quotes.md`](docs/sources/quotes.md).

## Scope

Hackathon v1 is frozen at the five markets above and their six sources
([`docs/hackathon-v1.md`](docs/hackathon-v1.md)). There are no other
markets, no streaming, no history, and no accounts, SDK or frontend.

## Development

```bash
./scripts/check.sh    # everything that must pass: fmt, clippy, Rust + TypeScript tests
./scripts/demo.sh     # end-to-end verification: seed, collect once, 22 API checks
```

`demo.sh` needs `DATABASE_URL`. For the NVDA quote, export
`APCA_API_KEY_ID` / `APCA_API_SECRET_KEY` (or put them in `.env`, which is
git-ignored). Database tests run when `DATABASE_URL` is set (the role needs
`CREATEDB`); CI requires them.

```bash
# Rust (in rust/)
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

# TypeScript (in typescript/)
bun install && bun run typecheck && bun run lint && bun test && bun run test
```

## Internals

For contributors; none of this is needed to use the API.

```text
undrly/
├── rust/crates/
│   ├── undrly-core/        canonical domain model (no I/O)
│   ├── undrly-store/       PostgreSQL migrations and repositories
│   ├── undrly-provider/    source adapters (the only network code)
│   ├── undrly-normalize/   source records → normalized values
│   ├── undrly-ingest/      raw-first ingestion, aggregation
│   └── undrly-collect/     collector binary (seed, poll)
├── typescript/
│   ├── apps/api/           read-only Hono API
│   ├── apps/mcp/           read-only MCP server (stdio) over the API routes
│   └── packages/contracts/ JSON API contract (Zod)
├── database/migrations/    PostgreSQL schema
├── data/demo/              curated demo universe
├── tests/fixtures/         captured source responses and contract fixtures
├── scripts/                dev.sh, present.sh, demo.sh, check.sh, worldsfair.sh, mcp.sh
└── docs/                   design notes
```

- Architecture source of truth: [`AGENT.md`](AGENT.md).
- Design notes: [`docs/domain.md`](docs/domain.md) (canonical model and
  identifiers), [`docs/persistence.md`](docs/persistence.md) (schema and
  provenance), [`docs/contracts.md`](docs/contracts.md) (API contract).
- Sources: [`docs/sources/quotes.md`](docs/sources/quotes.md),
  [`docs/sources/sec-edgar.md`](docs/sources/sec-edgar.md) (SEC EDGAR filer
  identity).
- Future architecture, not implemented:
  [`docs/phase-5-identity-resolution.md`](docs/phase-5-identity-resolution.md).
