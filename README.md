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
| Equity | `NVDA` | Alpaca (IEX feed) | the last trade on IEX: an IEX venue quote |
| Crypto spot | `BTC/USD` | Kraken + Coinbase | **one aggregate** of both venues' mid prices |
| FX | `EUR/USD` | Kraken | Kraken's last trade, with bid/ask |
| Commodity | `XAU/USD` | gold-api | an aggregated reference price per troy ounce |
| Perpetual | `BTC-PERP` | Hyperliquid | the mark price, in USDC |

Every answer has the same shape, whatever the market. This set is frozen for
the hackathon (see [Scope](#scope)).

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
| `/v1/instruments/{id}/graph` | an instrument's direct relationships and listings |

- **Queries:** a symbol or name (`NVDA`, `Gold`, `BTC perpetual`), a pair
  (`EUR/USD`), an identifier (`isin:US67066G1040`), a venue symbol
  (`NASDAQ:NVDA`), or an Undrly id.
- **Prices** are exact decimal strings (`"83839.5975000"`), never floats. The
  trailing digits are intentional.
- **Every quote** says where it comes from: `basis` (`venue` or `aggregated`),
  `venue`, `source`, the `unit` (a currency such as USD, or an asset such as
  USDC), `asOf` and `freshness`. NVDA shows `stale` outside US market hours;
  it is the last IEX trade.
- **Errors** are JSON: `bad_request` (400), `not_found` / `no_quote` (404),
  `ambiguous` (409).

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
  (`mean-venue-mid-v1`, 30 s freshness window). The venue observations stay
  available at `/v1/quotes/BTC/USD`.
- The **API** (TypeScript) never calls a provider while answering a request,
  so its speed doesn't depend on any upstream.
- Every quote traces back to the exact upstream response it came from.

## Data sources and licensing

Upstream data is used in **local/private demo mode only**. The
redistribution terms of every source (Kraken, Coinbase, Hyperliquid,
gold-api, Alpaca/IEX) are **unreviewed**, and Undrly does **not** currently
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
│   └── packages/contracts/ JSON API contract (Zod)
├── database/migrations/    PostgreSQL schema
├── data/demo/              curated demo universe
├── tests/fixtures/         captured source responses and contract fixtures
├── scripts/                dev.sh, present.sh, demo.sh, check.sh
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
