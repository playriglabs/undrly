//! `undrly-collect`: the cross-market demo data plane (docs/hackathon-v1.md).
//!
//! ```text
//! undrly-collect seed [path]     register sources, ingest curated reference data
//! undrly-collect run [--once]    poll quote sources sequentially, forever or once
//! ```
//!
//! Sources are polled **one request at a time**, each on its own conservative
//! interval, with no retries beyond the next tick and no concurrency. Every
//! response is stored raw first; its observations and the canonical quotes
//! they touch are written after.
//!
//! Environment: `DATABASE_URL` (required); `UNDRLY_USER_AGENT` (optional,
//! defaults to a generic Undrly agent); `APCA_API_KEY_ID` +
//! `APCA_API_SECRET_KEY` (optional: without them the Alpaca/IEX feed is
//! skipped). Credentials are sent as headers only; they are never stored or
//! printed.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use sqlx::PgConnection;
use sqlx::postgres::PgPoolOptions;
use undrly_core::{DisplayName, Redistribution, Source, SourceId, Timestamp};
use undrly_ingest::quotes::{QuoteIngestReport, ingest_quotes, refresh_canonical_quotes};
use undrly_ingest::{IngestError, RawRecord};
use undrly_normalize::alpaca::AlpacaNormalizer;
use undrly_normalize::coinbase::CoinbaseNormalizer;
use undrly_normalize::gold_api::GoldApiNormalizer;
use undrly_normalize::hyperliquid::HyperliquidNormalizer;
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_provider::alpaca::{self, AlpacaProvider, Credentials};
use undrly_provider::coinbase::{self, CoinbaseProvider};
use undrly_provider::gold_api::{self, GoldApiProvider};
use undrly_provider::http::{FetchedRecord, HttpClient};
use undrly_provider::hyperliquid::{self, HyperliquidProvider};
use undrly_provider::kraken::{self, KrakenProvider};
use undrly_store::market::quote_feeds_of_source;
use undrly_store::sources::insert_source;

const DEFAULT_USER_AGENT: &str = "Undrly/0.1 (+https://undrly.xyz)";
const DEFAULT_UNIVERSE: &str = "data/demo/universe.json";

/// Every demo source. Redistribution starts `unknown` (treated as
/// restricted) until a human reviews each source's terms.
const SOURCES: [(&str, &str); 6] = [
    ("undrly-curated", "Undrly curated reference data"),
    ("kraken", "Kraken"),
    ("hyperliquid", "Hyperliquid"),
    ("gold-api", "gold-api.com"),
    ("alpaca", "Alpaca Market Data (IEX feed)"),
    ("coinbase", "Coinbase Exchange"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Feed {
    Kraken,
    Coinbase,
    Hyperliquid,
    GoldApi,
    Alpaca,
}

impl Feed {
    const ALL: [Feed; 5] = [
        Feed::Kraken,
        Feed::Coinbase,
        Feed::Hyperliquid,
        Feed::GoldApi,
        Feed::Alpaca,
    ];

    fn source(self) -> &'static str {
        match self {
            Feed::Kraken => kraken::SOURCE_ID,
            Feed::Coinbase => coinbase::SOURCE_ID,
            Feed::Hyperliquid => hyperliquid::SOURCE_ID,
            Feed::GoldApi => gold_api::SOURCE_ID,
            Feed::Alpaca => alpaca::SOURCE_ID,
        }
    }

    /// Conservative polling intervals.
    fn interval(self) -> Duration {
        Duration::from_secs(match self {
            Feed::Kraken => 10,
            Feed::Coinbase => 10,
            Feed::Hyperliquid => 15,
            Feed::GoldApi => 60,
            Feed::Alpaca => 15,
        })
    }
}

#[derive(Debug)]
enum Error {
    Usage(String),
    Db(sqlx::Error),
    Store(undrly_store::StoreError),
    Ingest(IngestError),
    Fetch(undrly_provider::http::FetchError),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage(m) => write!(f, "{m}"),
            Error::Db(e) => write!(f, "database: {e}"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Ingest(e) => write!(f, "ingest: {e}"),
            Error::Fetch(e) => write!(f, "fetch: {e}"),
            Error::Io(e) => write!(f, "io: {e}"),
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
impl From<undrly_provider::http::FetchError> for Error {
    fn from(e: undrly_provider::http::FetchError) -> Self {
        Error::Fetch(e)
    }
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

async fn run(args: &[String]) -> Result<ExitCode, Error> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| Error::Usage("DATABASE_URL is not set".into()))?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    match args.first().map(String::as_str) {
        Some("seed") => {
            undrly_store::MIGRATOR
                .run(&pool)
                .await
                .map_err(|e| Error::Usage(format!("migrations: {e}")))?;
            let mut conn = pool.acquire().await?;
            seed(
                &mut conn,
                args.get(1).map_or(DEFAULT_UNIVERSE, String::as_str),
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Some("run") => {
            let once = args.iter().any(|a| a == "--once");
            let mut conn = pool.acquire().await?;
            collect(&mut conn, once).await
        }
        _ => Err(Error::Usage(
            "usage: undrly-collect seed [universe.json] | run [--once]".into(),
        )),
    }
}

async fn seed(conn: &mut PgConnection, path: &str) -> Result<(), Error> {
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
    let payload = std::fs::read(path).map_err(Error::Io)?;
    let report = undrly_ingest::curated::ingest_universe(
        conn,
        &RawRecord {
            record_key: path.to_owned(),
            payload,
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
    let feeds: Vec<Feed> = Feed::ALL
        .into_iter()
        .filter(|f| {
            let enabled = *f != Feed::Alpaca || credentials.is_some();
            if !enabled {
                println!("alpaca: skipped (APCA_API_KEY_ID / APCA_API_SECRET_KEY not set)");
            }
            enabled
        })
        .collect();
    let mut due: Vec<(Feed, Instant)> = feeds.iter().map(|f| (*f, Instant::now())).collect();
    let mut failures = 0;
    loop {
        for (feed, next) in due.iter_mut() {
            if Instant::now() < *next {
                continue;
            }
            *next = Instant::now() + feed.interval();
            if let Err(e) = poll(conn, &client, credentials.as_ref(), *feed).await {
                failures += 1;
                eprintln!("{}: error: {e}", feed.source());
            }
        }
        if once {
            return Ok(if failures == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        let wake = due
            .iter()
            .map(|(_, n)| *n)
            .min()
            .expect("at least one feed");
        tokio::time::sleep_until(wake.into()).await;
    }
}

/// One request to one source, ingested raw-first, then canonical quotes.
async fn poll(
    conn: &mut PgConnection,
    client: &HttpClient,
    credentials: Option<&Credentials>,
    feed: Feed,
) -> Result<(), Error> {
    let source = SourceId::parse(feed.source()).expect("valid source id");
    let symbols: Vec<String> = quote_feeds_of_source(conn, &source)
        .await?
        .into_iter()
        .map(|f| f.feed.symbol.as_str().to_owned())
        .collect();
    if symbols.is_empty() {
        println!("{source}: no quote feeds declared (run `seed` first)");
        return Ok(());
    }
    let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
    let fetched: FetchedRecord = match feed {
        Feed::Kraken => kraken::fetch_ticker(client, &symbols).await?,
        // One product per request: the book names no product.
        Feed::Coinbase => coinbase::fetch_book(client, symbols[0]).await?,
        Feed::Hyperliquid => hyperliquid::fetch_meta_and_asset_ctxs(client).await?,
        Feed::GoldApi => gold_api::fetch_price(client, symbols[0]).await?,
        Feed::Alpaca => {
            let credentials = credentials.expect("alpaca enabled only with credentials");
            alpaca::fetch_snapshots(client, credentials, &symbols).await?
        }
    };
    let raw = RawRecord {
        record_key: fetched.record_key.clone(),
        payload: fetched.body.clone(),
        received_at: fetched.received_at,
    };
    let report: QuoteIngestReport = match feed {
        Feed::Kraken => {
            ingest_quotes(conn, &KrakenProvider::new(), &KrakenNormalizer, &raw).await?
        }
        Feed::Coinbase => {
            ingest_quotes(conn, &CoinbaseProvider::new(), &CoinbaseNormalizer, &raw).await?
        }
        Feed::Hyperliquid => {
            ingest_quotes(
                conn,
                &HyperliquidProvider::new(),
                &HyperliquidNormalizer,
                &raw,
            )
            .await?
        }
        Feed::GoldApi => {
            ingest_quotes(conn, &GoldApiProvider::new(), &GoldApiNormalizer, &raw).await?
        }
        Feed::Alpaca => {
            ingest_quotes(conn, &AlpacaProvider::new(), &AlpacaNormalizer, &raw).await?
        }
    };
    let refreshed = refresh_canonical_quotes(conn, &report.pairs, Timestamp::now()).await?;
    let new = report
        .observations
        .iter()
        .filter(|(_, _, w)| *w == undrly_store::Write::Inserted)
        .count();
    println!(
        "{source}: {} bytes in {} ms; record {} ({:?}); {} observations ({new} new); {} canonical quotes{}",
        fetched.body.len(),
        fetched.elapsed.as_millis(),
        report.source_record.0.0,
        report.source_record.1,
        report.observations.len(),
        refreshed.len(),
        if report.missing.is_empty() {
            String::new()
        } else {
            format!("; missing {:?}", report.missing)
        },
    );
    Ok(())
}
