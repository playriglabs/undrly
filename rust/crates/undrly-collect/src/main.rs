//! `undrly-collect`: the cross-market data plane (docs/hackathon-v1.md,
//! docs/v1.1-universe.md).
//!
//! ```text
//! undrly-collect seed [path]        register sources; ingest the V1 curated universe, the
//!                                   curated commodities and, when built, the universe
//!                                   snapshot (or only `path`)
//! undrly-collect run [--once]       poll quote sources sequentially, forever or once
//! undrly-collect universe fetch     download raw universe files (the only networked step)
//! undrly-collect universe pools     GeckoTerminal pools of the Solana tokens (also run by fetch)
//! undrly-collect universe build     raw files + id map → snapshot + report (pure)
//! undrly-collect fx build           FX spec + id map → data/reference/fx.json + report (pure)
//! undrly-collect history [bars|reference|calendar|corporate-actions|economic|earnings|all] [--days-1h N] [--days-1d N] [--source ID]
//!                                   backfill bars, reference series and the equity calendar
//! undrly-collect onchain            chains and issuer deployments from their own sources
//!                                   (data/reference/onchain.json; V1.5)
//! ```
//!
//! Sources are polled **one request at a time**, each on its own conservative
//! interval, with no retries beyond the next tick and no concurrency. Every
//! response is stored raw first; its observations and the canonical quotes
//! they touch are written after.
//!
//! Environment: `DATABASE_URL` (seed, run); `UNDRLY_USER_AGENT` (optional);
//! `APCA_API_KEY_ID` + `APCA_API_SECRET_KEY` (optional: Alpaca/IEX);
//! `EIA_API_KEY` (optional: EIA); `UNDRLY_SEC_USER_AGENT` (universe fetch).
//! Credentials are sent in headers (Alpaca) or the request URL (EIA) only;
//! they are never stored or printed.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use sqlx::PgConnection;
use sqlx::postgres::PgPoolOptions;
use undrly_core::{
    DisplayName, PriceType, Redistribution, Source, SourceId, Timestamp, VenueSymbol,
};
use undrly_ingest::market_data::{ingest_perp_contexts, ingest_reference_window};
use undrly_ingest::quotes::{
    QuoteIngestReport, ingest_quotes_for, ingest_quotes_of_types, refresh_canonical_quotes,
};
use undrly_ingest::{IngestError, RawRecord, store_raw_record};
use undrly_normalize::alpaca::AlpacaNormalizer;
use undrly_normalize::coinbase::CoinbaseNormalizer;
use undrly_normalize::eia::EiaNormalizer;
use undrly_normalize::fx::{
    BankIndonesiaNormalizer, BankOfCanadaNormalizer, BitstampNormalizer, BnmNormalizer,
    CbmNormalizer, EcbNormalizer, FedH10Normalizer,
};
use undrly_normalize::gold_api::GoldApiNormalizer;
use undrly_normalize::hyperliquid::{HyperliquidBookNormalizer, HyperliquidNormalizer};
use undrly_normalize::jupiter::JupiterNormalizer;
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_normalize::venues::{
    BitkubNormalizer, BookTickerNormalizer, HashKeyNormalizer, IndodaxNormalizer, OkxNormalizer,
};
use undrly_normalize::worldbank::WorldBankNormalizer;
use undrly_provider::alpaca::{self, AlpacaProvider, Credentials};
use undrly_provider::bank_indonesia::{self, BankIndonesiaProvider};
use undrly_provider::bank_of_canada::{self, BankOfCanadaProvider};
use undrly_provider::binance::{self, BinanceProvider};
use undrly_provider::bitkub::{self, BitkubProvider};
use undrly_provider::bitstamp::{self, BitstampProvider};
use undrly_provider::bnm::{self, BnmProvider};
use undrly_provider::cbm::{self, CbmProvider};
use undrly_provider::coinbase::{self, CoinbaseProvider};
use undrly_provider::coins_ph::{self, CoinsPhProvider};
use undrly_provider::curated::CuratedProvider;
use undrly_provider::ecb::{self, EcbProvider};
use undrly_provider::eia::{self, EiaProvider};
use undrly_provider::fed_h10::{self, FedH10Provider};
use undrly_provider::gold_api::{self, GoldApiProvider};
use undrly_provider::hashkey::{self, HashKeyProvider};
use undrly_provider::http::{FetchError, FetchedRecord, HttpClient};
use undrly_provider::hyperliquid::{self, HyperliquidBookProvider, HyperliquidProvider};
use undrly_provider::indodax::{self, IndodaxProvider};
use undrly_provider::jupiter::{self, JupiterProvider};
use undrly_provider::kraken::{self, KrakenProvider};
use undrly_provider::okx::{self, OkxProvider};
use undrly_provider::sec::http::{SecClient, SecUserAgent};
use undrly_provider::worldbank::{self, WorldBankProvider};
use undrly_provider::{
    backed, backpack, binance_bstocks, coingecko, geckoterminal, invesco, rhj, sec, ssga,
};
use undrly_store::market::quote_feeds_of_source;
use undrly_store::sources::insert_source;
use undrly_universe::{IdMap, Inputs, Manifest, ManifestFile, paths, sha256_hex, to_json};

const DEFAULT_USER_AGENT: &str = "Undrly/0.1 (+https://undrly.xyz)";
const DEFAULT_UNIVERSE: &str = "data/demo/universe.json";
const COMMODITIES: &str = "data/reference/commodities.json";
const FX: &str = "data/reference/fx.json";
const FX_IDS: &str = "data/reference/fx-ids.json";
const FX_REPORT: &str = "data/reference/fx-report.md";
const STABLECOIN_FX: &str = "data/reference/stablecoin-fx.json";
const STABLECOIN_FX_IDS: &str = "data/reference/stablecoin-fx-ids.json";
const STABLECOIN_FX_REPORT: &str = "data/reference/stablecoin-fx-report.md";
const UNIVERSE_DIR: &str = "data/universe";

/// Every source. Redistribution starts `unknown` (treated as restricted)
/// until a human reviews each source's terms.
const SOURCES: [(&str, &str); 41] = [
    ("undrly-curated", "Undrly curated reference data"),
    (
        "undrly-universe",
        "Undrly universe snapshot (generated locally)",
    ),
    ("kraken", "Kraken"),
    ("hyperliquid", "Hyperliquid"),
    ("gold-api", "gold-api.com"),
    ("alpaca", "Alpaca Market Data (IEX feed)"),
    ("coinbase", "Coinbase Exchange"),
    ("eia", "U.S. Energy Information Administration"),
    ("worldbank", "World Bank Commodity Price Data (Pink Sheet)"),
    ("coingecko", "CoinGecko"),
    ("ssga", "State Street SPDR SPY holdings"),
    ("nasdaq", "Nasdaq.com"),
    ("sec-edgar", "SEC EDGAR"),
    ("bitstamp", "Bitstamp"),
    ("ecb", "European Central Bank euro reference rates"),
    (
        "bank-of-canada",
        "Bank of Canada daily exchange rates (Valet)",
    ),
    ("fed-h10", "Federal Reserve H.10 foreign exchange rates"),
    (
        "bank-indonesia",
        "Bank Indonesia (JISDOR and BI transaction rates)",
    ),
    ("bnm", "Bank Negara Malaysia exchange rates (Open API)"),
    ("cbm", "Central Bank of Myanmar reference exchange rates"),
    (
        "fred",
        "FRED (Federal Reserve Bank of St. Louis) release calendar",
    ),
    ("finnhub", "Finnhub earnings calendar"),
    (
        "circle",
        "Circle developer documentation (USDC contract addresses)",
    ),
    (
        "solana-mainnet-rpc",
        "Solana mainnet RPC (api.mainnet.solana.com)",
    ),
    (
        "robinhood-chain-rpc",
        "Robinhood Chain mainnet RPC (rpc.mainnet.chain.robinhood.com)",
    ),
    (
        "rhj-api",
        "Robinhood Assets (Jersey) Limited asset registry (api.robinhood.com/rhj)",
    ),
    (
        "rhj-final-terms",
        "Robinhood Assets (Jersey) Limited Final Terms (documents)",
    ),
    ("tempo-rpc", "Tempo Mainnet RPC (rpc.tempo.xyz)"),
    ("binance", "Binance spot public market data"),
    ("okx", "OKX public market data"),
    ("indodax", "Indodax public market data"),
    ("bitkub", "Bitkub public market data"),
    ("coins-ph", "Coins.ph (Coins Pro) public market data"),
    ("hashkey", "HashKey Exchange public market data"),
    ("invesco", "Invesco QQQ ETF holdings"),
    (
        "backed-api",
        "Backed Assets (JE) Limited xStocks registry (api.backed.fi)",
    ),
    (
        "backpack",
        "Backpack Exchange public API (Backpack Securities)",
    ),
    ("jupiter", "Jupiter Price API (Solana tokens by mint)"),
    ("binance-bstocks", "Binance bStocks list (binance.com)"),
    (
        "geckoterminal",
        "GeckoTerminal public API: Solana DEX pools and their OHLC",
    ),
    (
        "bnb-chain-rpc",
        "BNB Chain mainnet RPC (bsc-dataseed.bnbchain.org)",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Feed {
    Kraken,
    Coinbase,
    /// Hyperliquid marks (`metaAndAssetCtxs`, one request for every perp).
    Hyperliquid,
    /// Hyperliquid order books (`l2Book`, one perp per request).
    HyperliquidBook,
    GoldApi,
    /// gold-api's OHLC over the last 24 hours (API key, V1.9).
    GoldApiOhlc,
    Alpaca,
    Eia,
    WorldBank,
    Bitstamp,
    Ecb,
    BankOfCanada,
    FedH10,
    BankIndonesia,
    Bnm,
    Cbm,
    /// V1.9 stablecoin/fiat venue books.
    Binance,
    Okx,
    Indodax,
    Bitkub,
    CoinsPh,
    HashKey,
    /// V1.10: tokenized stocks on Solana, by mint.
    Jupiter,
}

impl Feed {
    const ALL: [Feed; 23] = [
        Feed::Kraken,
        Feed::Coinbase,
        Feed::Hyperliquid,
        Feed::HyperliquidBook,
        Feed::GoldApi,
        Feed::GoldApiOhlc,
        Feed::Alpaca,
        Feed::Eia,
        Feed::WorldBank,
        Feed::Bitstamp,
        Feed::Ecb,
        Feed::BankOfCanada,
        Feed::FedH10,
        Feed::BankIndonesia,
        Feed::Bnm,
        Feed::Cbm,
        Feed::Binance,
        Feed::Okx,
        Feed::Indodax,
        Feed::Bitkub,
        Feed::CoinsPh,
        Feed::HashKey,
        Feed::Jupiter,
    ];

    fn source(self) -> &'static str {
        match self {
            Feed::Kraken => kraken::SOURCE_ID,
            Feed::Coinbase => coinbase::SOURCE_ID,
            Feed::Hyperliquid | Feed::HyperliquidBook => hyperliquid::SOURCE_ID,
            Feed::GoldApi | Feed::GoldApiOhlc => gold_api::SOURCE_ID,
            Feed::Alpaca => alpaca::SOURCE_ID,
            Feed::Eia => eia::SOURCE_ID,
            Feed::WorldBank => worldbank::SOURCE_ID,
            Feed::Bitstamp => bitstamp::SOURCE_ID,
            Feed::Ecb => ecb::SOURCE_ID,
            Feed::BankOfCanada => bank_of_canada::SOURCE_ID,
            Feed::FedH10 => fed_h10::SOURCE_ID,
            Feed::BankIndonesia => bank_indonesia::SOURCE_ID,
            Feed::Bnm => bnm::SOURCE_ID,
            Feed::Cbm => cbm::SOURCE_ID,
            Feed::Binance => binance::SOURCE_ID,
            Feed::Okx => okx::SOURCE_ID,
            Feed::Indodax => indodax::SOURCE_ID,
            Feed::Bitkub => bitkub::SOURCE_ID,
            Feed::CoinsPh => coins_ph::SOURCE_ID,
            Feed::HashKey => hashkey::SOURCE_ID,
            Feed::Jupiter => jupiter::SOURCE_ID,
        }
    }

    /// Conservative polling intervals (docs/v1.1-universe.md §5).
    fn interval(self) -> Duration {
        Duration::from_secs(match self {
            Feed::Kraken => 10,
            Feed::Coinbase => 10,
            Feed::Hyperliquid => 15,
            // A sweep of every perpetual's book, restarted once a minute:
            // 178 requests of weight 2 against Hyperliquid's 1,200 per minute.
            Feed::HyperliquidBook => 60,
            Feed::GoldApi => 60,
            // Free tier: 10 history/OHLC requests an hour; four metals hourly.
            Feed::GoldApiOhlc => 3600,
            Feed::Alpaca => 30,
            Feed::Eia => 6 * 3600,
            Feed::WorldBank => 24 * 3600,
            Feed::Bitstamp => 10,
            // Stablecoin/fiat books (docs/v1.9-live-fx.md).
            Feed::Binance
            | Feed::Okx
            | Feed::Indodax
            | Feed::Bitkub
            | Feed::CoinsPh
            | Feed::HashKey => 10,
            // Daily reference rates (docs/v1.2-fx.md §6); an unchanged
            // response is the same raw record.
            Feed::Ecb | Feed::BankOfCanada | Feed::BankIndonesia | Feed::Bnm | Feed::Cbm => 3600,
            // Published weekly.
            Feed::FedH10 => 6 * 3600,
            // Onchain trading moves slower than a venue book; a handful of
            // requests a minute, well inside the free tier.
            Feed::Jupiter => 60,
        })
    }

    /// The symbol batches one poll requests, one record each.
    fn batches(self, symbols: &[String]) -> Vec<Vec<String>> {
        match self {
            // One request covers every symbol.
            Feed::Hyperliquid
            | Feed::WorldBank
            | Feed::Ecb
            | Feed::FedH10
            | Feed::Bnm
            | Feed::Cbm => vec![symbols.to_vec()],
            // One product / metal / market / series per request.
            Feed::Coinbase
            | Feed::HyperliquidBook
            | Feed::GoldApi
            | Feed::GoldApiOhlc
            | Feed::Bitstamp
            | Feed::BankIndonesia
            | Feed::Okx
            | Feed::Indodax
            | Feed::Bitkub
            | Feed::CoinsPh
            | Feed::HashKey => symbols.iter().map(|s| vec![s.clone()]).collect(),
            // Up to the record key's 2,048 characters since V1.10 (about 120
            // symbols a request), for its ~500 USDT markets.
            Feed::Binance => pack_to(symbols, BINANCE_URL_MAX, |b| {
                BinanceProvider::book_ticker_url(b).len()
            }),
            // 40 mints per request (Jupiter allows 50): the record key (the
            // URL) stays within source_records' 2,048 characters.
            Feed::Jupiter => symbols
                .chunks(JUPITER_BATCH)
                .map(<[String]>::to_vec)
                .collect(),
            Feed::BankOfCanada => {
                pack(symbols, |b| BankOfCanadaProvider::observations_url(b).len())
            }
            // As many symbols per request as the record key (the URL) allows.
            Feed::Kraken => pack(symbols, |b| KrakenProvider::ticker_url(b).len()),
            Feed::Alpaca => pack(symbols, |b| AlpacaProvider::snapshots_url(b).len()),
            // One request per API route.
            Feed::Eia => {
                let mut by_route: BTreeMap<&str, Vec<String>> = BTreeMap::new();
                for s in symbols {
                    by_route
                        .entry(eia::route_of(s).unwrap_or("unknown"))
                        .or_default()
                        .push(s.clone());
                }
                by_route.into_values().collect()
            }
        }
    }

    /// Pause between the requests of one poll.
    fn spacing(self) -> Duration {
        match self {
            Feed::Coinbase => Duration::from_millis(150),
            // Keeps a sweep of every book under a minute, so each book stays
            // close to the latest mark (MARK_BOOK_MAX_SKEW_SECONDS).
            Feed::HyperliquidBook => Duration::from_millis(50),
            // Kraken's public API allows about one request per second.
            Feed::Kraken | Feed::Jupiter => Duration::from_secs(1),
            _ => Duration::from_millis(50),
        }
    }
}

impl Feed {
    /// The price types this feed's responses carry, when a source has
    /// several requests for the same symbols (Hyperliquid: marks and books).
    fn price_types(self) -> Option<&'static [PriceType]> {
        match self {
            Feed::Hyperliquid => Some(&[PriceType::Mark]),
            Feed::HyperliquidBook => Some(&[PriceType::Mid]),
            _ => None,
        }
    }
}

/// The URL length Kraken and Alpaca batches are packed to (V1.1; the
/// `source_records.record_key` limit was 256 until V1.10 raised it).
const RECORD_KEY_MAX: usize = 256;

/// Mints per Jupiter request.
const JUPITER_BATCH: usize = 40;

/// Binance spot book-ticker URLs are packed to this length (V1.10).
const BINANCE_URL_MAX: usize = 2000;

/// Greedy batches in order, each with a URL of at most [`RECORD_KEY_MAX`].
fn pack(symbols: &[String], url_len: impl Fn(&[&str]) -> usize) -> Vec<Vec<String>> {
    pack_to(symbols, RECORD_KEY_MAX, url_len)
}

/// Greedy batches in order, each with a URL of at most `max` characters.
fn pack_to(symbols: &[String], max: usize, url_len: impl Fn(&[&str]) -> usize) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for s in symbols {
        current.push(s);
        if current.len() > 1 && url_len(&current) > max {
            current.pop();
            out.push(current.iter().map(|s| (*s).to_owned()).collect());
            current = vec![s];
        }
    }
    if !current.is_empty() {
        out.push(current.iter().map(|s| (*s).to_owned()).collect());
    }
    out
}

#[derive(Debug)]
enum Error {
    Usage(String),
    Db(sqlx::Error),
    Store(undrly_store::StoreError),
    Ingest(IngestError),
    Fetch(FetchError),
    Io(String, std::io::Error),
    Build(undrly_universe::BuildError),
    Json(String, serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage(m) => write!(f, "{m}"),
            Error::Db(e) => write!(f, "database: {e}"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Ingest(e) => write!(f, "ingest: {e}"),
            Error::Fetch(e) => write!(f, "fetch: {e}"),
            Error::Io(p, e) => write!(f, "{p}: {e}"),
            Error::Build(e) => write!(f, "universe build: {e}"),
            Error::Json(p, e) => write!(f, "{p}: {e}"),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::Db(e)
    }
}
impl From<undrly_store::StoreError> for Error {
    fn from(e: undrly_store::StoreError) -> Self {
        Error::Store(e)
    }
}
impl From<IngestError> for Error {
    fn from(e: IngestError) -> Self {
        Error::Ingest(e)
    }
}
impl From<FetchError> for Error {
    fn from(e: FetchError) -> Self {
        Error::Fetch(e)
    }
}
impl From<undrly_universe::BuildError> for Error {
    fn from(e: undrly_universe::BuildError) -> Self {
        Error::Build(e)
    }
}

fn read(path: impl AsRef<Path>) -> Result<Vec<u8>, Error> {
    let path = path.as_ref();
    std::fs::read(path).map_err(|e| Error::Io(path.display().to_string(), e))
}

fn write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), Error> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(dir.display().to_string(), e))?;
    }
    std::fs::write(path, bytes).map_err(|e| Error::Io(path.display().to_string(), e))
}

fn json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, Error> {
    let path = path.as_ref();
    serde_json::from_slice(&read(path)?).map_err(|e| Error::Json(path.display().to_string(), e))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: undrly-collect seed [curated.json] | run [--once] | universe fetch | universe pools | universe build | fx build | history [bars|reference|calendar|corporate-actions|economic|earnings|all] [--days-1h N] [--days-1d N] [--source ID] | onchain";

async fn run(args: &[String]) -> Result<ExitCode, Error> {
    let command: Vec<&str> = args.iter().map(String::as_str).collect();
    match command.as_slice() {
        ["universe", "fetch"] => return universe_fetch(Path::new(UNIVERSE_DIR)).await,
        ["universe", "build"] => return universe_build(Path::new(UNIVERSE_DIR)),
        ["universe", "pools"] => {
            let user_agent = std::env::var("UNDRLY_USER_AGENT")
                .unwrap_or_else(|_| DEFAULT_USER_AGENT.to_owned());
            let client = HttpClient::new(&user_agent)?;
            return universe_pools(Path::new(UNIVERSE_DIR), &client).await;
        }
        ["fx", "build"] => return fx_build(),
        ["seed", ..] | ["run", ..] | ["history", ..] | ["onchain"] => {}
        _ => return Err(Error::Usage(USAGE.into())),
    }
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| Error::Usage("DATABASE_URL is not set".into()))?;
    // `run` refreshes bars on a second connection (V1.10).
    let live = command[0] == "run" && !command.contains(&"--once");
    let pool = PgPoolOptions::new()
        .max_connections(if live { 2 } else { 1 })
        .connect(&url)
        .await?;
    if command[0] == "seed" {
        undrly_store::MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| Error::Usage(format!("migrations: {e}")))?;
    }
    let mut conn = pool.acquire().await?;
    match command.as_slice() {
        ["seed", rest @ ..] => {
            seed(&mut conn, rest.first().copied()).await?;
            Ok(ExitCode::SUCCESS)
        }
        ["run", rest @ ..] => {
            if live {
                tokio::spawn(history::live_bars(pool.clone()));
            }
            collect(&mut conn, rest.contains(&"--once")).await
        }
        ["history", rest @ ..] => history::run(&mut conn, rest).await,
        ["onchain"] => {
            register_sources(&mut conn).await?;
            let user_agent = std::env::var("UNDRLY_USER_AGENT")
                .unwrap_or_else(|_| DEFAULT_USER_AGENT.to_owned());
            onchain::run(&mut conn, &HttpClient::new(&user_agent)?).await?;
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(Error::Usage(USAGE.into())),
    }
}

// ---------------------------------------------------------------- seed

async fn register_sources(conn: &mut PgConnection) -> Result<(), Error> {
    for (id, name) in SOURCES {
        insert_source(
            conn,
            &Source {
                id: SourceId::parse(id).expect("valid source id"),
                name: DisplayName::new(name).expect("valid name"),
                redistribution: Redistribution::Unknown,
            },
        )
        .await?;
    }
    Ok(())
}

async fn seed_curated(
    conn: &mut PgConnection,
    provider: CuratedProvider,
    path: &str,
) -> Result<(), Error> {
    let report = undrly_ingest::curated::ingest_universe_as(
        conn,
        provider,
        &RawRecord {
            record_key: path.to_owned(),
            payload: read(path)?,
            received_at: Timestamp::now(),
        },
    )
    .await?;
    println!(
        "seed: {path}: {} facts inserted, {} unchanged",
        report.inserted, report.unchanged
    );
    Ok(())
}

async fn seed(conn: &mut PgConnection, path: Option<&str>) -> Result<(), Error> {
    register_sources(conn).await?;
    if let Some(path) = path {
        return seed_curated(conn, CuratedProvider::new(), path).await;
    }
    seed_curated(conn, CuratedProvider::new(), DEFAULT_UNIVERSE).await?;
    seed_curated(conn, CuratedProvider::new(), COMMODITIES).await?;
    seed_fx(conn).await?;
    // Before the snapshot, which reuses its EURC (STABLECOIN_PINS).
    seed_stablecoin_fx(conn).await?;
    seed_universe(conn).await
}

/// The V1.1 universe snapshot, its raw upstream files first.
async fn seed_universe(conn: &mut PgConnection) -> Result<(), Error> {
    let root = Path::new(UNIVERSE_DIR);
    let snapshot = root.join("snapshot.json");
    if !snapshot.exists() {
        println!(
            "seed: no {} (run `universe fetch` and `universe build` for V1.1)",
            snapshot.display()
        );
        return Ok(());
    }
    // Raw upstream files first: memberships name them.
    let manifest: Manifest = json(root.join("manifest.json"))?;
    let mut stored = 0;
    for f in &manifest.files {
        let payload = read(root.join("raw").join(&f.path))?;
        if sha256_hex(&payload) != f.sha256 {
            return Err(Error::Usage(format!(
                "{}: SHA-256 differs from the manifest",
                f.path
            )));
        }
        let received_at = Timestamp::parse(&f.fetched_at)
            .map_err(|e| Error::Usage(format!("{}: fetchedAt: {e}", f.path)))?;
        let mut tx = sqlx::Acquire::begin(&mut *conn).await?;
        store_raw_record(
            &mut tx,
            &SourceId::parse(&f.source).map_err(|e| Error::Usage(e.to_string()))?,
            &RawRecord {
                record_key: f.record_key.clone(),
                payload,
                received_at,
            },
        )
        .await?;
        tx.commit().await?;
        stored += 1;
    }
    println!("seed: {stored} raw universe files stored");
    seed_curated(
        conn,
        CuratedProvider::with_source(
            SourceId::parse(undrly_universe::SNAPSHOT_SOURCE).expect("valid source id"),
        ),
        &snapshot.display().to_string(),
    )
    .await
}

/// Stablecoin/fiat markets and derived FX crosses (docs/v1.9-live-fx.md),
/// after the FX file it references.
async fn seed_stablecoin_fx(conn: &mut PgConnection) -> Result<(), Error> {
    if !Path::new(STABLECOIN_FX).exists() {
        println!("seed: no {STABLECOIN_FX} (run `undrly-collect fx build`)");
        return Ok(());
    }
    seed_curated(conn, CuratedProvider::new(), STABLECOIN_FX).await
}

/// The FX universes (docs/v1.2-fx.md): the spec first, as its own raw
/// record (the memberships name it), then the built curated file.
async fn seed_fx(conn: &mut PgConnection) -> Result<(), Error> {
    if !Path::new(FX).exists() {
        println!("seed: no {FX} (run `undrly-collect fx build`)");
        return Ok(());
    }
    let spec = undrly_universe::fx::SPEC_PATH;
    let mut tx = sqlx::Acquire::begin(&mut *conn).await?;
    store_raw_record(
        &mut tx,
        &SourceId::parse(undrly_universe::fx::SPEC_SOURCE).expect("valid source id"),
        &RawRecord {
            record_key: spec.to_owned(),
            payload: read(spec)?,
            received_at: Timestamp::now(),
        },
    )
    .await?;
    tx.commit().await?;
    seed_curated(conn, CuratedProvider::new(), FX).await
}

fn fx_build() -> Result<ExitCode, Error> {
    let v1 = serde_json::from_slice(&read(DEFAULT_UNIVERSE)?)
        .map_err(|e| Error::Json(DEFAULT_UNIVERSE.into(), e))?;
    let ids: IdMap = if Path::new(FX_IDS).exists() {
        json(FX_IDS)?
    } else {
        IdMap::default()
    };
    let spec = read(undrly_universe::fx::SPEC_PATH)?;
    let build = undrly_universe::fx::build(&spec, &v1, ids, &mut undrly_universe::mint)?;
    write(FX_IDS, &to_json(&build.ids))?;
    write(FX, &to_json(&build.universe))?;
    write(FX_REPORT, build.report.as_bytes())?;
    let u = &build.universe;
    println!(
        "fx build: {} new currencies, {} new FX instruments, {} feeds; members {:?}; {} ids minted",
        u.currencies.len(),
        u.instruments.len(),
        u.quote_feeds.len(),
        u.universes
            .iter()
            .map(|x| (x.key.as_str(), x.members.len()))
            .collect::<Vec<_>>(),
        build.minted
    );
    println!("  report: {FX_REPORT}");

    // V1.9: stablecoin/fiat markets and derived crosses, on the FX ids.
    let stable_ids: IdMap = if Path::new(STABLECOIN_FX_IDS).exists() {
        json(STABLECOIN_FX_IDS)?
    } else {
        IdMap::default()
    };
    let spec = read(undrly_universe::stablecoin_fx::SPEC_PATH)?;
    let stable = undrly_universe::stablecoin_fx::build(
        &spec,
        &v1,
        &build.ids,
        stable_ids,
        &mut undrly_universe::mint,
    )?;
    write(STABLECOIN_FX_IDS, &to_json(&stable.ids))?;
    write(STABLECOIN_FX, &to_json(&stable.universe))?;
    write(STABLECOIN_FX_REPORT, stable.report.as_bytes())?;
    let u = &stable.universe;
    println!(
        "stablecoin fx build: {} venues, {} feeds, {} aggregations, {} derived crosses; {} ids minted",
        u.venues.len(),
        u.quote_feeds.len(),
        u.quote_aggregations.len(),
        u.quote_derivations.len(),
        stable.minted
    );
    println!("  report: {STABLECOIN_FX_REPORT}");
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- universe

/// CoinGecko's public API: at most one request per 7 s; on HTTP 429 wait a
/// minute and retry (five times at most).
struct Paced {
    last: Option<Instant>,
}

impl Paced {
    async fn get(&mut self, client: &HttpClient, url: &str) -> Result<FetchedRecord, Error> {
        for attempt in 0..6 {
            if let Some(last) = self.last {
                tokio::time::sleep_until((last + Duration::from_secs(7)).into()).await;
            }
            self.last = Some(Instant::now());
            match client.get(url, &[]).await {
                Err(FetchError::Status { status: 429, .. }) if attempt < 5 => {
                    println!("  coingecko: HTTP 429, waiting 60 s");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                other => return Ok(other?),
            }
        }
        unreachable!("the last attempt returns")
    }
}

/// A GeckoTerminal GET within its free tier: on HTTP 429, a minute's wait
/// before trying again, at most five times. Callers space requests
/// [`geckoterminal::REQUEST_SPACING_MS`] apart.
async fn gecko_get(client: &HttpClient, url: &str) -> Result<FetchedRecord, Error> {
    for attempt in 0..6 {
        match client.get(url, &[]).await {
            Err(FetchError::Status { status: 429, .. }) if attempt < 5 => {
                println!("  geckoterminal: HTTP 429, waiting 60 s");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            other => return Ok(other?),
        }
    }
    unreachable!("the last attempt returns")
}

struct Fetcher {
    raw: PathBuf,
    files: Vec<ManifestFile>,
}

impl Fetcher {
    fn keep(&mut self, path: &str, source: &str, f: &FetchedRecord) -> Result<(), Error> {
        write(self.raw.join(path), &f.body)?;
        println!("  {path}: {} bytes", f.body.len());
        self.files.push(ManifestFile {
            path: path.to_owned(),
            source: source.to_owned(),
            record_key: f.record_key.clone(),
            sha256: sha256_hex(&f.body),
            bytes: f.body.len(),
            fetched_at: f.received_at.to_string(),
        });
        Ok(())
    }
}

async fn universe_fetch(root: &Path) -> Result<ExitCode, Error> {
    let sec_ua = std::env::var("UNDRLY_SEC_USER_AGENT").map_err(|_| {
        Error::Usage(
            "UNDRLY_SEC_USER_AGENT is not set (SEC requires a requester name and contact email)"
                .into(),
        )
    })?;
    let sec = SecClient::new(SecUserAgent::new(&sec_ua)?)?;
    let user_agent =
        std::env::var("UNDRLY_USER_AGENT").unwrap_or_else(|_| DEFAULT_USER_AGENT.to_owned());
    let client = HttpClient::new(&user_agent)?;
    let raw = root.join("raw");
    if raw.exists() {
        std::fs::remove_dir_all(&raw).map_err(|e| Error::Io(raw.display().to_string(), e))?;
    }
    let mut out = Fetcher {
        raw,
        files: Vec::new(),
    };
    let mut cg = Paced { last: None };
    println!("universe fetch → {}", root.display());

    let f = cg.get(&client, &coingecko::markets_url(1)).await?;
    out.keep(paths::MARKETS, coingecko::SOURCE_ID, &f)?;
    let f = cg.get(&client, &coingecko::markets_url(2)).await?;
    out.keep(paths::MARKETS_PAGE2, coingecko::SOURCE_ID, &f)?;
    let f = cg.get(&client, &coingecko::coins_list_url()).await?;
    out.keep(paths::COINS_LIST, coingecko::SOURCE_ID, &f)?;
    for (exchange, prefix) in [
        ("kraken", paths::KRAKEN_TICKERS),
        ("gdax", paths::COINBASE_TICKERS),
        ("binance", paths::BINANCE_TICKERS),
    ] {
        for page in 1..=100u32 {
            let f = cg
                .get(&client, &coingecko::exchange_tickers_url(exchange, page))
                .await?;
            let n = coingecko::decode_tickers(&f.body)
                .map_err(|e| Error::Usage(e.to_string()))?
                .tickers
                .len();
            out.keep(&format!("{prefix}{page:03}.json"), coingecko::SOURCE_ID, &f)?;
            if n < coingecko::TICKERS_PER_PAGE {
                break;
            }
        }
    }
    let f = cg
        .get(&client, &coingecko::derivatives_url("hyperliquid"))
        .await?;
    out.keep(paths::HYPERLIQUID_DERIVATIVES, coingecko::SOURCE_ID, &f)?;

    let f = client.get(kraken::ASSET_PAIRS_URL, &[]).await?;
    out.keep(paths::KRAKEN_ASSET_PAIRS, kraken::SOURCE_ID, &f)?;
    let f = client.get(coinbase::BASE_URL, &[]).await?;
    out.keep(paths::COINBASE_PRODUCTS, coinbase::SOURCE_ID, &f)?;
    let f = hyperliquid::fetch_meta_and_asset_ctxs(&client).await?;
    out.keep(paths::HYPERLIQUID_META, hyperliquid::SOURCE_ID, &f)?;
    let f = client.get(ssga::SPY_HOLDINGS_URL, &[]).await?;
    out.keep(paths::SPY_HOLDINGS, ssga::SOURCE_ID, &f)?;
    let f = client.get(ssga::MDY_HOLDINGS_URL, &[]).await?;
    out.keep(paths::MDY_HOLDINGS, ssga::SOURCE_ID, &f)?;
    let f = client.get(ssga::SPSM_HOLDINGS_URL, &[]).await?;
    out.keep(paths::SPSM_HOLDINGS, ssga::SOURCE_ID, &f)?;
    // Nasdaq-100 through QQQ's holdings (V1.10).
    let f = client.get(invesco::QQQ_HOLDINGS_URL, &[]).await?;
    out.keep(paths::QQQ_HOLDINGS, invesco::SOURCE_ID, &f)?;
    // Tokenized stocks: each issuer's own registry (V1.10).
    let f = client.get(backed::TOKENS_URL, &[]).await?;
    out.keep(paths::BACKED_TOKENS, backed::SOURCE_ID, &f)?;
    let f = client.get(backpack::SECURITIES_URL, &[]).await?;
    out.keep(paths::BACKPACK_SECURITIES, backpack::SOURCE_ID, &f)?;
    let f = client.get(backpack::ASSETS_URL, &[]).await?;
    out.keep(paths::BACKPACK_ASSETS, backpack::SOURCE_ID, &f)?;
    let f = rhj::fetch_assets(&client).await?;
    out.keep(paths::RHJ_ASSETS, rhj::API_SOURCE_ID, &f)?;
    let f = client.get(binance_bstocks::LIST_URL, &[]).await?;
    out.keep(paths::BSTOCKS, binance_bstocks::SOURCE_ID, &f)?;
    let f = client.get(binance::EXCHANGE_INFO_URL, &[]).await?;
    out.keep(paths::BINANCE_EXCHANGE_INFO, binance::SOURCE_ID, &f)?;
    let f = sec.fetch_company_tickers_exchange().await?;
    out.keep(paths::SEC_TICKERS, sec::SOURCE_ID, &f)?;

    out.files.sort_by(|a, b| a.path.cmp(&b.path));
    write(
        root.join("manifest.json"),
        &to_json(&Manifest { files: out.files }),
    )?;
    // Onchain pools of the Solana tokens this fetch prices (V1.10).
    universe_pools(root, &client).await?;
    println!("universe fetch: done; next: undrly-collect universe build");
    Ok(ExitCode::SUCCESS)
}

fn universe_build(root: &Path) -> Result<ExitCode, Error> {
    let manifest: Manifest = json(root.join("manifest.json"))?;
    let build = build_in_memory(root, &manifest)?;
    let ids_path = root.join("ids.json");
    write(&ids_path, &to_json(&build.ids))?;
    write(root.join("snapshot.json"), &to_json(&build.snapshot))?;
    write(root.join("deployments.json"), &to_json(&build.deployments))?;
    write(root.join("report.md"), build.report.as_bytes())?;
    let c = &build.coverage;
    println!(
        "universe build: {} new instruments, {} issuer entities, {} new feeds, {} deployments; members {:?}; exclusions {:?}",
        c.instruments_new,
        c.entities_new,
        c.feeds_new,
        build.deployments.deployments.len(),
        c.members,
        c.excluded
    );
    println!("  report: {}", root.join("report.md").display());
    Ok(ExitCode::SUCCESS)
}

/// The universe build of `manifest`'s raw files, nothing written.
fn build_in_memory(root: &Path, manifest: &Manifest) -> Result<undrly_universe::Build, Error> {
    let mut files = BTreeMap::new();
    for f in &manifest.files {
        files.insert(f.path.clone(), read(root.join("raw").join(&f.path))?);
    }
    let v1 = serde_json::from_slice(&read(DEFAULT_UNIVERSE)?)
        .map_err(|e| Error::Json(DEFAULT_UNIVERSE.into(), e))?;
    let ids_path = root.join("ids.json");
    let ids: IdMap = if ids_path.exists() {
        json(&ids_path)?
    } else {
        IdMap::default()
    };
    let bound = onchain::bound_underlyings()?;
    // Crypto assets the stablecoin FX file declared first (V1.9).
    let stable_ids: IdMap = if Path::new(STABLECOIN_FX_IDS).exists() {
        json(STABLECOIN_FX_IDS)?
    } else {
        IdMap::default()
    };
    let external: BTreeMap<String, String> = undrly_universe::STABLECOIN_PINS
        .iter()
        .filter_map(|(key, theirs)| Some(((*key).to_owned(), stable_ids.ids.get(*theirs)?.clone())))
        .collect();
    Ok(undrly_universe::build(
        &Inputs {
            manifest,
            files: &files,
            v1: &v1,
            ids,
            bound_underlyings: &bound,
            external: &external,
        },
        &mut undrly_universe::mint,
    )?)
}

/// GeckoTerminal's top pools for every Solana token the build prices
/// through Jupiter, and the DEX names (V1.10, docs/v1.10-tokenized-stocks.md
/// §13). Replaces the manifest's earlier GeckoTerminal files; the next
/// `universe build` declares one pool feed per token.
async fn universe_pools(root: &Path, client: &HttpClient) -> Result<ExitCode, Error> {
    let mut manifest: Manifest = json(root.join("manifest.json"))?;
    manifest
        .files
        .retain(|f| f.source != geckoterminal::SOURCE_ID);
    let build = build_in_memory(root, &manifest)?;
    let mut mints: Vec<&str> = build
        .snapshot
        .quote_feeds
        .iter()
        .filter(|f| f.source == jupiter::SOURCE_ID)
        .map(|f| f.symbol.as_str())
        .collect();
    mints.sort_unstable();
    mints.dedup();
    let mut out = Fetcher {
        raw: root.join("raw"),
        files: manifest.files,
    };
    let dir = out.raw.join("geckoterminal");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| Error::Io(dir.display().to_string(), e))?;
    }
    // Only tokens Jupiter prices now are charted: the price is the filter,
    // not stored (the collector stores prices).
    let mut priced: Vec<&str> = Vec::new();
    for batch in mints.chunks(jupiter::MAX_IDS) {
        let f = jupiter::fetch_prices(client, batch).await?;
        let prices = undrly_provider::QuoteProvider::decode_quote(&JupiterProvider::new(), &f.body)
            .map_err(|e| Error::Usage(e.to_string()))?;
        priced.extend(
            batch
                .iter()
                .filter(|m| prices.0.get(**m).is_some_and(|p| p.usd_price.is_some())),
        );
        tokio::time::sleep(Duration::from_millis(1_000)).await;
    }
    println!(
        "universe pools: {} Solana tokens, {} priced by Jupiter now; {} requests",
        mints.len(),
        priced.len(),
        priced.len() + 1
    );
    let pace = Duration::from_millis(geckoterminal::REQUEST_SPACING_MS);
    for mint in &priced {
        let f = gecko_get(client, &geckoterminal::pools_url(mint)).await?;
        out.keep(
            &format!("{}{mint}.json", paths::GECKOTERMINAL_POOLS),
            geckoterminal::SOURCE_ID,
            &f,
        )?;
        tokio::time::sleep(pace).await;
    }
    let f = gecko_get(client, &geckoterminal::dexes_url(1)).await?;
    out.keep(paths::GECKOTERMINAL_DEXES, geckoterminal::SOURCE_ID, &f)?;
    out.files.sort_by(|a, b| a.path.cmp(&b.path));
    write(
        root.join("manifest.json"),
        &to_json(&Manifest { files: out.files }),
    )?;
    println!("universe pools: done; next: undrly-collect universe build");
    Ok(ExitCode::SUCCESS)
}

mod history;
mod onchain;

// ---------------------------------------------------------------- run

async fn collect(conn: &mut PgConnection, once: bool) -> Result<ExitCode, Error> {
    let user_agent =
        std::env::var("UNDRLY_USER_AGENT").unwrap_or_else(|_| DEFAULT_USER_AGENT.to_owned());
    let client = HttpClient::new(&user_agent)?;
    let credentials = match (
        std::env::var("APCA_API_KEY_ID"),
        std::env::var("APCA_API_SECRET_KEY"),
    ) {
        (Ok(key_id), Ok(secret_key)) => Some(Credentials { key_id, secret_key }),
        _ => None,
    };
    let eia_key = std::env::var("EIA_API_KEY").ok().filter(|k| !k.is_empty());
    let gold_key = std::env::var("GOLD_API_KEY").ok().filter(|k| !k.is_empty());
    let feeds: Vec<Feed> = Feed::ALL
        .into_iter()
        .filter(|f| match f {
            Feed::Alpaca if credentials.is_none() => {
                println!("alpaca: skipped (APCA_API_KEY_ID / APCA_API_SECRET_KEY not set)");
                false
            }
            Feed::Eia if eia_key.is_none() => {
                println!("eia: skipped (EIA_API_KEY not set)");
                false
            }
            Feed::GoldApiOhlc if gold_key.is_none() => {
                println!("gold-api ohlc: skipped (GOLD_API_KEY not set)");
                false
            }
            _ => true,
        })
        .collect();
    let keys = Keys {
        alpaca: credentials.as_ref(),
        eia: eia_key.as_deref(),
        gold: gold_key.as_deref(),
    };
    if once {
        return once_pass(conn, &client, &keys, &feeds).await;
    }
    // Continuous: one request at a time. A source's poll is a queue of
    // batches; between two requests the scheduler serves whichever source is
    // due first, so a long sweep (Coinbase, one product per request) never
    // holds back a fast source (Kraken, every 10 s).
    let mut slots: Vec<Slot> = feeds
        .iter()
        .map(|f| Slot {
            feed: *f,
            next: Instant::now(),
            started: Instant::now(),
            pending: VecDeque::new(),
            tally: Tally::default(),
        })
        .collect();
    loop {
        let slot = slots
            .iter_mut()
            .min_by_key(|s| s.next)
            .expect("at least one feed");
        tokio::time::sleep_until(slot.next.into()).await;
        if slot.pending.is_empty() {
            let symbols = symbols_of(conn, slot.feed).await?;
            if symbols.is_empty() {
                println!(
                    "{}: no quote feeds declared (run `seed` first)",
                    slot.feed.source()
                );
                slot.next = Instant::now() + slot.feed.interval();
                continue;
            }
            slot.pending = slot.feed.batches(&symbols).into();
            slot.started = Instant::now();
            slot.tally = Tally::default();
        }
        let batch = slot.pending.pop_front().expect("non-empty");
        request(conn, &client, &keys, slot.feed, &batch, &mut slot.tally).await?;
        slot.next = if slot.pending.is_empty() {
            slot.tally.print(slot.feed, slot.started);
            (slot.started + slot.feed.interval()).max(Instant::now())
        } else {
            Instant::now() + slot.feed.spacing()
        };
    }
}

/// `run --once`: each source once, slowest-moving first and the venue pair
/// (Coinbase, then Kraken) last, with the Coinbase sweep in reverse
/// declaration order, so the pass ends with the V1 pairs' venue quotes fresh
/// for their 30 s aggregation window. Later-declared pairs may end the pass
/// with a single-venue aggregate.
async fn once_pass(
    conn: &mut PgConnection,
    client: &HttpClient,
    keys: &Keys<'_>,
    feeds: &[Feed],
) -> Result<ExitCode, Error> {
    const ORDER: [Feed; 23] = [
        Feed::WorldBank,
        Feed::Eia,
        Feed::FedH10,
        Feed::Ecb,
        Feed::BankOfCanada,
        Feed::BankIndonesia,
        Feed::Bnm,
        Feed::Cbm,
        Feed::GoldApi,
        Feed::GoldApiOhlc,
        Feed::Alpaca,
        Feed::Jupiter,
        // Books first, then the marks they are attached to (within 60 s).
        Feed::HyperliquidBook,
        Feed::Hyperliquid,
        Feed::Coinbase,
        Feed::Bitstamp,
        // Stablecoin/fiat legs before Kraken's USD legs, so a cross sees both fresh.
        Feed::Binance,
        Feed::Okx,
        Feed::Indodax,
        Feed::Bitkub,
        Feed::CoinsPh,
        Feed::HashKey,
        Feed::Kraken,
    ];
    let mut failures = 0;
    for feed in ORDER.into_iter().filter(|f| feeds.contains(f)) {
        let mut symbols = symbols_of(conn, feed).await?;
        if symbols.is_empty() {
            println!(
                "{}: no quote feeds declared (run `seed` first)",
                feed.source()
            );
            continue;
        }
        if feed == Feed::Coinbase {
            symbols.reverse();
        }
        let started = Instant::now();
        let mut tally = Tally::default();
        for (i, batch) in feed.batches(&symbols).into_iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(feed.spacing()).await;
            }
            request(conn, client, keys, feed, &batch, &mut tally).await?;
        }
        tally.print(feed, started);
        failures += tally.failures;
    }
    Ok(if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

struct Keys<'a> {
    alpaca: Option<&'a Credentials>,
    eia: Option<&'a str>,
    gold: Option<&'a str>,
}

/// A source's progress through its current poll.
struct Slot {
    feed: Feed,
    next: Instant,
    started: Instant,
    pending: VecDeque<Vec<String>>,
    tally: Tally,
}

#[derive(Default)]
struct Tally {
    records: usize,
    bytes: usize,
    observations: usize,
    new: usize,
    canonical: usize,
    missing: Vec<String>,
    failures: usize,
}

impl Tally {
    fn print(&self, feed: Feed, started: Instant) {
        println!(
            "{}: {} records, {} bytes in {} ms; {} observations ({} new); {} canonical quotes{}{}",
            feed.source(),
            self.records,
            self.bytes,
            started.elapsed().as_millis(),
            self.observations,
            self.new,
            self.canonical,
            if self.missing.is_empty() {
                String::new()
            } else {
                format!("; missing {:?}", self.missing)
            },
            if self.failures == 0 {
                String::new()
            } else {
                format!("; {} failed requests", self.failures)
            },
        );
    }
}

/// The distinct feed symbols of a source, in declaration order.
async fn symbols_of(conn: &mut PgConnection, feed: Feed) -> Result<Vec<String>, Error> {
    let source = SourceId::parse(feed.source()).expect("valid source id");
    let mut symbols: Vec<String> = Vec::new();
    for f in quote_feeds_of_source(conn, &source).await? {
        if feed
            .price_types()
            .is_some_and(|types| !types.contains(&f.feed.price_type))
        {
            continue;
        }
        let s = f.feed.symbol.as_str().to_owned();
        if !symbols.contains(&s) {
            symbols.push(s);
        }
    }
    Ok(symbols)
}

/// One request; a failure is logged and counted, not fatal.
async fn request(
    conn: &mut PgConnection,
    client: &HttpClient,
    keys: &Keys<'_>,
    feed: Feed,
    batch: &[String],
    tally: &mut Tally,
) -> Result<(), Error> {
    if let Err(e) = poll_batch(conn, client, keys, feed, batch, tally).await {
        tally.failures += 1;
        eprintln!("{}: error ({}): {e}", feed.source(), batch.join(","));
    }
    Ok(())
}

/// One request of one source: fetched, stored raw first, ingested for
/// exactly its symbols, then its canonical quotes.
async fn poll_batch(
    conn: &mut PgConnection,
    client: &HttpClient,
    keys: &Keys<'_>,
    feed: Feed,
    batch: &[String],
    tally: &mut Tally,
) -> Result<(), Error> {
    let refs: Vec<&str> = batch.iter().map(String::as_str).collect();
    let fetched: FetchedRecord = match feed {
        Feed::Kraken => kraken::fetch_ticker(client, &refs).await?,
        // One product per request: the book names no product.
        Feed::Coinbase => coinbase::fetch_book(client, refs[0]).await?,
        Feed::Hyperliquid => hyperliquid::fetch_meta_and_asset_ctxs(client).await?,
        Feed::HyperliquidBook => hyperliquid::fetch_l2_book(client, refs[0]).await?,
        Feed::GoldApi => gold_api::fetch_price(client, refs[0]).await?,
        Feed::GoldApiOhlc => {
            let key = keys.gold.expect("gold-api ohlc enabled only with a key");
            let end = chrono::Utc::now().timestamp();
            gold_api::fetch_ohlc(client, key, refs[0], end - 86_400, end).await?
        }
        Feed::Alpaca => {
            let credentials = keys.alpaca.expect("alpaca enabled only with credentials");
            alpaca::fetch_snapshots(client, credentials, &refs).await?
        }
        Feed::Eia => {
            let key = keys.eia.expect("eia enabled only with a key");
            let route = eia::route_of(refs[0])
                .ok_or_else(|| Error::Usage(format!("no EIA route for {}", refs[0])))?;
            eia::fetch_series(client, key, route, &refs).await?
        }
        Feed::WorldBank => worldbank::fetch_monthly_workbook(client).await?,
        Feed::Bitstamp => bitstamp::fetch_ticker(client, refs[0]).await?,
        Feed::Ecb => ecb::fetch_daily(client).await?,
        Feed::BankOfCanada => bank_of_canada::fetch_observations(client, &refs).await?,
        Feed::FedH10 => fed_h10::fetch_package(client).await?,
        Feed::BankIndonesia => {
            // The last 14 days, in Jakarta's calendar (UTC+7).
            let today = (chrono::Utc::now() + chrono::TimeDelta::hours(7)).date_naive();
            let start = today - chrono::TimeDelta::days(14);
            let url = BankIndonesiaProvider::rates_url(
                refs[0],
                &start.format("%Y-%m-%d").to_string(),
                &today.format("%Y-%m-%d").to_string(),
            )
            .ok_or_else(|| Error::Usage(format!("no Bank Indonesia series for {}", refs[0])))?;
            bank_indonesia::fetch_rates(client, &url).await?
        }
        Feed::Bnm => bnm::fetch_rates(client).await?,
        Feed::Cbm => cbm::fetch_latest(client).await?,
        Feed::Binance => binance::fetch_book_tickers(client, &refs).await?,
        Feed::Okx => okx::fetch_ticker(client, refs[0]).await?,
        Feed::Indodax => indodax::fetch_ticker(client, refs[0]).await?,
        Feed::Bitkub => bitkub::fetch_ticker(client, refs[0]).await?,
        Feed::CoinsPh => coins_ph::fetch_book_ticker(client, refs[0]).await?,
        Feed::HashKey => hashkey::fetch_book_ticker(client, refs[0]).await?,
        Feed::Jupiter => jupiter::fetch_prices(client, &refs).await?,
    };
    let raw = RawRecord {
        record_key: fetched.record_key.clone(),
        payload: fetched.body.clone(),
        received_at: fetched.received_at,
    };
    if feed == Feed::GoldApiOhlc {
        // A window, not quotes: stored as the source states it (V1.9).
        let symbol = VenueSymbol::new(&batch[0]).map_err(|e| Error::Usage(e.to_string()))?;
        let write = ingest_reference_window(conn, &raw, &symbol).await?;
        tally.records += 1;
        tally.bytes += fetched.body.len();
        tally.new += usize::from(write == undrly_store::Write::Inserted);
        return Ok(());
    }
    let requested: Vec<VenueSymbol> = batch
        .iter()
        .filter_map(|s| VenueSymbol::new(s).ok())
        .collect();
    if feed == Feed::Hyperliquid {
        // The same response carries each perpetual's context (V1.3).
        ingest_perp_contexts(conn, &raw, &requested).await?;
    }
    let requested = Some(requested.as_slice());
    let report: QuoteIngestReport = match feed {
        Feed::Kraken => {
            ingest_quotes_for(
                conn,
                &KrakenProvider::new(),
                &KrakenNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Coinbase => {
            ingest_quotes_for(
                conn,
                &CoinbaseProvider::new(),
                &CoinbaseNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Hyperliquid => {
            ingest_quotes_of_types(
                conn,
                &HyperliquidProvider::new(),
                &HyperliquidNormalizer,
                &raw,
                requested,
                feed.price_types(),
            )
            .await?
        }
        Feed::HyperliquidBook => {
            ingest_quotes_of_types(
                conn,
                &HyperliquidBookProvider::new(),
                &HyperliquidBookNormalizer,
                &raw,
                requested,
                feed.price_types(),
            )
            .await?
        }
        // Stored as a window and returned above.
        Feed::GoldApiOhlc => unreachable!("gold-api ohlc is not a quote feed"),
        Feed::GoldApi => {
            ingest_quotes_for(
                conn,
                &GoldApiProvider::new(),
                &GoldApiNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Alpaca => {
            ingest_quotes_for(
                conn,
                &AlpacaProvider::new(),
                &AlpacaNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Eia => {
            ingest_quotes_for(conn, &EiaProvider::new(), &EiaNormalizer, &raw, requested).await?
        }
        Feed::WorldBank => {
            ingest_quotes_for(
                conn,
                &WorldBankProvider::new(),
                &WorldBankNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Bitstamp => {
            ingest_quotes_for(
                conn,
                &BitstampProvider::new(),
                &BitstampNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Ecb => {
            ingest_quotes_for(conn, &EcbProvider::new(), &EcbNormalizer, &raw, requested).await?
        }
        Feed::BankOfCanada => {
            ingest_quotes_for(
                conn,
                &BankOfCanadaProvider::new(),
                &BankOfCanadaNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::FedH10 => {
            ingest_quotes_for(
                conn,
                &FedH10Provider::new(),
                &FedH10Normalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::BankIndonesia => {
            ingest_quotes_for(
                conn,
                &BankIndonesiaProvider::new(),
                &BankIndonesiaNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Bnm => {
            ingest_quotes_for(conn, &BnmProvider::new(), &BnmNormalizer, &raw, requested).await?
        }
        Feed::Cbm => {
            ingest_quotes_for(conn, &CbmProvider::new(), &CbmNormalizer, &raw, requested).await?
        }
        Feed::Binance => {
            ingest_quotes_for(
                conn,
                &BinanceProvider::new(),
                &BookTickerNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Okx => {
            ingest_quotes_for(conn, &OkxProvider::new(), &OkxNormalizer, &raw, requested).await?
        }
        Feed::Indodax => {
            ingest_quotes_for(
                conn,
                &IndodaxProvider::new(),
                &IndodaxNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Bitkub => {
            ingest_quotes_for(
                conn,
                &BitkubProvider::new(),
                &BitkubNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::CoinsPh => {
            ingest_quotes_for(
                conn,
                &CoinsPhProvider::new(),
                &BookTickerNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::HashKey => {
            ingest_quotes_for(
                conn,
                &HashKeyProvider::new(),
                &HashKeyNormalizer,
                &raw,
                requested,
            )
            .await?
        }
        Feed::Jupiter => {
            ingest_quotes_for(
                conn,
                &JupiterProvider::new(),
                &JupiterNormalizer,
                &raw,
                requested,
            )
            .await?
        }
    };
    let refreshed = refresh_canonical_quotes(conn, &report.pairs, Timestamp::now()).await?;
    tally.records += 1;
    tally.bytes += fetched.body.len();
    tally.observations += report.observations.len();
    tally.new += report
        .observations
        .iter()
        .filter(|(_, _, w)| *w == undrly_store::Write::Inserted)
        .count();
    tally.canonical += refreshed.iter().filter(|r| r.write.is_some()).count();
    tally
        .missing
        .extend(report.missing.iter().map(|s| s.as_str().to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_fit_the_record_key() {
        let symbols: Vec<String> = (0..200).map(|i| format!("SYM{i:04}USD")).collect();
        for feed in [Feed::Kraken, Feed::Alpaca] {
            let batches = feed.batches(&symbols);
            assert!(batches.len() > 1);
            assert_eq!(batches.concat(), symbols, "every symbol once, in order");
            for b in &batches {
                let refs: Vec<&str> = b.iter().map(String::as_str).collect();
                let url = match feed {
                    Feed::Kraken => KrakenProvider::ticker_url(&refs),
                    _ => AlpacaProvider::snapshots_url(&refs),
                };
                assert!(url.len() <= RECORD_KEY_MAX, "{url}");
            }
        }
        let eia = Feed::Eia.batches(&["RWTC".into(), "RNGWHHD".into(), "RBRTE".into()]);
        assert_eq!(
            eia,
            vec![
                vec!["RNGWHHD".to_owned()],
                vec!["RWTC".to_owned(), "RBRTE".to_owned()]
            ]
        );
    }
}
