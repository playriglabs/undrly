//! `undrly-collect history`: V1.3 backfill (docs/v1.3-market-data.md §7).
//!
//! ```text
//! history bars       venue OHLC bars (1h, 1d): Kraken (crypto spot + Kraken FX),
//!                    Hyperliquid (perpetuals), Alpaca IEX (equities)
//! history reference  reference-series history: ECB (90 days), Bank of Canada,
//!                    Fed H.10, Bank Indonesia, BNM (3 months), CBM (60 days),
//!                    EIA (90 values), World Bank (every month)
//! history calendar   the US equity trading calendar (IEX sessions)
//! history corporate-actions
//!                    equities' dividends, splits, spin-offs, mergers, name changes
//!                    (Alpaca; a year back and six months ahead)
//! history economic   scheduled dates of curated US economic releases (FRED;
//!                    FRED_API_KEY; a month back and six months ahead)
//! history earnings   equities' earnings dates and estimates (Finnhub;
//!                    FINNHUB_API_KEY; a month back and three months ahead)
//! history all        all of the above (default)
//! ```
//!
//! Same rules as `run`: sequential, raw-first, conservative spacing, no
//! retries beyond the next invocation. Re-running is idempotent: identical
//! responses are the same raw records, and bars/observations deduplicate.
//! The API never calls a provider; it reads what this stores.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, TimeDelta, Utc};
use sqlx::PgConnection;
use undrly_core::{BarInterval, ObservationBasis, SourceId, VenueId, VenueSymbol};
use undrly_ingest::RawRecord;
use undrly_ingest::market_data::{
    BarIngestReport, ingest_bars, ingest_calendar, ingest_corporate_actions, ingest_earnings,
    ingest_economic_release_dates,
};
use undrly_ingest::quotes::{QuoteIngestReport, ingest_history_for, ingest_quotes_for};
use undrly_normalize::eia::EiaNormalizer;
use undrly_normalize::fx::{
    BankIndonesiaNormalizer, BankOfCanadaNormalizer, BnmMonthNormalizer, CbmNormalizer,
    EcbNormalizer, FedH10Normalizer,
};
use undrly_normalize::market_data::{
    AlpacaBarNormalizer, BitkubBarNormalizer, HyperliquidBarNormalizer, IndodaxBarNormalizer,
    KlineBarNormalizer, KrakenBarNormalizer, OkxBarNormalizer,
};
use undrly_normalize::worldbank::WorldBankNormalizer;
use undrly_provider::alpaca::{self, AlpacaProvider, Credentials};
use undrly_provider::bank_indonesia::{self, BankIndonesiaProvider};
use undrly_provider::bank_of_canada::{self, BankOfCanadaProvider};
use undrly_provider::binance::{self, BinanceProvider};
use undrly_provider::bitkub::{self, BitkubProvider};
use undrly_provider::bnm::{self, BnmMonthProvider};
use undrly_provider::cbm::{self, CbmProvider};
use undrly_provider::coins_ph::{self, CoinsPhProvider};
use undrly_provider::ecb::{self, EcbProvider};
use undrly_provider::eia::{self, EiaProvider};
use undrly_provider::fed_h10::{self, FedH10Provider};
use undrly_provider::hashkey::{self, HashKeyProvider};
use undrly_provider::http::{FetchedRecord, HttpClient};
use undrly_provider::hyperliquid::{self, HyperliquidProvider};
use undrly_provider::indodax::{self, IndodaxProvider};
use undrly_provider::kraken::{self, KrakenProvider};
use undrly_provider::okx::{self, OkxProvider};
use undrly_provider::worldbank::{self, WorldBankProvider};
use undrly_provider::{finnhub, fred};
use undrly_store::market::quote_feeds_of_source;

use super::{DEFAULT_USER_AGENT, Error, RECORD_KEY_MAX, pack};

struct Options {
    days_1h: i64,
    days_1d: i64,
}

fn parse(args: &[&str]) -> Result<(Vec<&'static str>, Options), Error> {
    let mut what = Vec::new();
    let mut o = Options {
        days_1h: 30,
        days_1d: 365,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "bars" => what.push("bars"),
            "reference" => what.push("reference"),
            "calendar" => what.push("calendar"),
            "corporate-actions" => what.push("corporate-actions"),
            "economic" => what.push("economic"),
            "earnings" => what.push("earnings"),
            "all" => what.extend([
                "calendar",
                "corporate-actions",
                "economic",
                "earnings",
                "reference",
                "bars",
            ]),
            "--days-1h" | "--days-1d" => {
                let n: i64 = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| (1..=3650).contains(n))
                    .ok_or_else(|| Error::Usage(format!("{a} needs 1..=3650")))?;
                if *a == "--days-1h" {
                    o.days_1h = n;
                } else {
                    o.days_1d = n;
                }
            }
            other => return Err(Error::Usage(format!("history: unknown argument `{other}`"))),
        }
    }
    if what.is_empty() {
        what.extend([
            "calendar",
            "corporate-actions",
            "economic",
            "earnings",
            "reference",
            "bars",
        ]);
    }
    Ok((what, o))
}

/// Per-step tally, printed as one line.
#[derive(Default)]
struct Tally {
    records: usize,
    bytes: usize,
    inserted: usize,
    replaced: usize,
    unchanged: usize,
    failures: usize,
}

impl Tally {
    fn bars(&mut self, f: &FetchedRecord, r: &BarIngestReport) {
        self.records += 1;
        self.bytes += f.body.len();
        self.inserted += r.inserted;
        self.replaced += r.replaced;
        self.unchanged += r.unchanged;
    }

    fn quotes(&mut self, f: &FetchedRecord, r: &QuoteIngestReport) {
        self.records += 1;
        self.bytes += f.body.len();
        for (_, _, w) in &r.observations {
            match w {
                undrly_store::Write::Inserted => self.inserted += 1,
                undrly_store::Write::Unchanged => self.unchanged += 1,
            }
        }
    }

    fn print(&self, what: &str, started: Instant) {
        println!(
            "history {what}: {} records, {} bytes in {} ms; {} new, {} replaced, {} unchanged{}",
            self.records,
            self.bytes,
            started.elapsed().as_millis(),
            self.inserted,
            self.replaced,
            self.unchanged,
            if self.failures == 0 {
                String::new()
            } else {
                format!("; {} failed requests", self.failures)
            }
        );
    }
}

fn raw(f: &FetchedRecord) -> RawRecord {
    RawRecord {
        record_key: f.record_key.clone(),
        payload: f.body.clone(),
        received_at: f.received_at,
    }
}

fn sym(s: &str) -> Vec<VenueSymbol> {
    VenueSymbol::new(s).map(|v| vec![v]).unwrap_or_default()
}

/// The source's feed symbols, in declaration order (venue feeds only when
/// `venue_only`), and the venue of the first venue feed.
async fn symbols(
    conn: &mut PgConnection,
    source: &str,
    venue_only: bool,
) -> Result<(Vec<String>, Option<VenueId>), Error> {
    let source = SourceId::parse(source).expect("valid source id");
    let mut out: Vec<String> = Vec::new();
    let mut venue = None;
    for f in quote_feeds_of_source(conn, &source).await? {
        let is_venue = matches!(f.feed.basis, ObservationBasis::Venue(_));
        if let ObservationBasis::Venue(v) = f.feed.basis {
            venue.get_or_insert(v);
        }
        if venue_only && !is_venue {
            continue;
        }
        let s = f.feed.symbol.as_str().to_owned();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    Ok((out, venue))
}

pub(super) async fn run(conn: &mut PgConnection, args: &[&str]) -> Result<ExitCode, Error> {
    let (what, o) = parse(args)?;
    let user_agent =
        std::env::var("UNDRLY_USER_AGENT").unwrap_or_else(|_| DEFAULT_USER_AGENT.to_owned());
    let client = HttpClient::new(&user_agent)?;
    let alpaca = match (
        std::env::var("APCA_API_KEY_ID"),
        std::env::var("APCA_API_SECRET_KEY"),
    ) {
        (Ok(key_id), Ok(secret_key)) => Some(Credentials { key_id, secret_key }),
        _ => None,
    };
    let eia_key = std::env::var("EIA_API_KEY").ok().filter(|k| !k.is_empty());
    let started = Instant::now();
    let mut failures = 0;
    for w in what {
        failures += match w {
            "calendar" => calendar(conn, &client, alpaca.as_ref()).await?,
            "corporate-actions" => corporate_actions(conn, &client, alpaca.as_ref()).await?,
            "economic" => economic(conn, &client).await?,
            "earnings" => earnings(conn, &client).await?,
            "reference" => reference(conn, &client, eia_key.as_deref()).await?,
            _ => bars(conn, &client, alpaca.as_ref(), &o).await?,
        };
    }
    println!("history: done in {} ms", started.elapsed().as_millis());
    Ok(if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Runs one request; logs and counts a failure instead of stopping.
macro_rules! step {
    ($tally:expr, $label:expr, $body:expr) => {
        match async { $body }.await {
            Ok(()) => {}
            Err(e) => {
                $tally.failures += 1;
                eprintln!("history: {}: {e}", $label);
            }
        }
    };
}

// ---------------------------------------------------------------- bars

async fn bars(
    conn: &mut PgConnection,
    client: &HttpClient,
    alpaca: Option<&Credentials>,
    o: &Options,
) -> Result<usize, Error> {
    let now = Utc::now();
    let mut failures = 0;

    // Kraken: one pair and interval per request (at most 720 bars), 1 req/s.
    let (pairs, _) = symbols(conn, kraken::SOURCE_ID, true).await?;
    let started = Instant::now();
    let mut t = Tally::default();
    for pair in &pairs {
        for (interval, minutes, days) in [
            (BarInterval::OneHour, 60, o.days_1h),
            (BarInterval::OneDay, 1440, o.days_1d),
        ] {
            let since = (now - TimeDelta::days(days)).timestamp();
            step!(t, format!("kraken {pair} {}", interval.as_str()), {
                let f = kraken::fetch_ohlc(client, pair, minutes, Some(since)).await?;
                let r = ingest_bars(
                    conn,
                    &KrakenProvider::new(),
                    &KrakenBarNormalizer(interval),
                    &raw(&f),
                    &sym(pair),
                )
                .await?;
                t.bars(&f, &r);
                Ok::<(), Error>(())
            });
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    t.print("bars kraken", started);
    failures += t.failures;

    // Hyperliquid: one coin and interval per request; spaced for the info
    // endpoint's weight limit.
    let (coins, _) = symbols(conn, hyperliquid::SOURCE_ID, true).await?;
    let started = Instant::now();
    let mut t = Tally::default();
    let end = now.timestamp_millis();
    for coin in &coins {
        for (interval, days) in [("1h", o.days_1h), ("1d", o.days_1d)] {
            let start = (now - TimeDelta::days(days)).timestamp_millis();
            step!(t, format!("hyperliquid {coin} {interval}"), {
                let f = hyperliquid::fetch_candles(client, coin, interval, start, end).await?;
                let r = ingest_bars(
                    conn,
                    &HyperliquidProvider::new(),
                    &HyperliquidBarNormalizer,
                    &raw(&f),
                    &sym(coin),
                )
                .await?;
                t.bars(&f, &r);
                Ok::<(), Error>(())
            });
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    }
    t.print("bars hyperliquid", started);
    failures += t.failures;

    failures += stablecoin_venue_bars(conn, client, o).await?;

    // Alpaca IEX: batches of symbols per URL, every page followed.
    let Some(credentials) = alpaca else {
        println!("history bars alpaca: skipped (APCA_API_KEY_ID / APCA_API_SECRET_KEY not set)");
        return Ok(failures);
    };
    let (stocks, _) = symbols(conn, alpaca::SOURCE_ID, true).await?;
    let started = Instant::now();
    let mut t = Tally::default();
    for (interval, timeframe, days) in [
        (BarInterval::OneHour, "1Hour", o.days_1h),
        (BarInterval::OneDay, "1Day", o.days_1d),
    ] {
        let start = (now - TimeDelta::days(days))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
        // Room for a page token in the record key.
        let token = "x".repeat(48);
        let batches = pack(&stocks, |b| {
            AlpacaProvider::bars_url(b, timeframe, &start, Some(&token)).len()
        });
        for batch in batches {
            let refs: Vec<&str> = batch.iter().map(String::as_str).collect();
            let requested: Vec<VenueSymbol> = batch
                .iter()
                .filter_map(|s| VenueSymbol::new(s).ok())
                .collect();
            let mut page: Option<String> = None;
            loop {
                let url = AlpacaProvider::bars_url(&refs, timeframe, &start, page.as_deref());
                if url.len() > RECORD_KEY_MAX {
                    return Err(Error::Usage(format!("alpaca bars URL too long: {url}")));
                }
                let mut next = None;
                step!(t, format!("alpaca {timeframe} {}", batch.join(",")), {
                    let f = alpaca::fetch_authenticated(client, credentials, &url).await?;
                    let r = ingest_bars(
                        conn,
                        &AlpacaProvider::new(),
                        &AlpacaBarNormalizer(interval),
                        &raw(&f),
                        &requested,
                    )
                    .await?;
                    t.bars(&f, &r);
                    let decoded: alpaca::BarsPage = serde_json::from_slice(&f.body)
                        .map_err(|e| Error::Json("alpaca bars".into(), e))?;
                    next = decoded.next_page_token;
                    Ok::<(), Error>(())
                });
                tokio::time::sleep(Duration::from_millis(350)).await;
                match next {
                    Some(p) => page = Some(p),
                    None => break,
                }
            }
        }
    }
    t.print("bars alpaca", started);
    Ok(failures + t.failures)
}

// ---------------------------------------------------------------- calendar

async fn calendar(
    conn: &mut PgConnection,
    client: &HttpClient,
    alpaca: Option<&Credentials>,
) -> Result<usize, Error> {
    let Some(credentials) = alpaca else {
        println!("history calendar: skipped (Alpaca credentials not set)");
        return Ok(0);
    };
    let (_, venue) = symbols(conn, alpaca::SOURCE_ID, true).await?;
    let Some(venue) = venue else {
        println!("history calendar: no Alpaca venue feed (run `seed` first)");
        return Ok(0);
    };
    let today = Utc::now().date_naive();
    let (first, last) = (today - TimeDelta::days(60), today + TimeDelta::days(90));
    let started = Instant::now();
    let mut t = Tally::default();
    step!(t, "calendar", {
        let url = AlpacaProvider::calendar_url(&first.to_string(), &last.to_string());
        let f = alpaca::fetch_authenticated(client, credentials, &url).await?;
        let n = ingest_calendar(
            conn,
            &SourceId::parse(alpaca::SOURCE_ID).expect("valid source id"),
            &raw(&f),
            venue,
            first,
            last,
        )
        .await?;
        t.records += 1;
        t.bytes += f.body.len();
        t.inserted += n;
        Ok::<(), Error>(())
    });
    t.print(&format!("calendar {first}..{last}"), started);
    Ok(t.failures)
}

// ---------------------------------------------------------------- corporate actions

async fn corporate_actions(
    conn: &mut PgConnection,
    client: &HttpClient,
    alpaca: Option<&Credentials>,
) -> Result<usize, Error> {
    let Some(credentials) = alpaca else {
        println!("history corporate-actions: skipped (Alpaca credentials not set)");
        return Ok(0);
    };
    let (stocks, _) = symbols(conn, alpaca::SOURCE_ID, true).await?;
    let today = Utc::now().date_naive();
    let (start, end) = (
        (today - TimeDelta::days(365)).to_string(),
        (today + TimeDelta::days(180)).to_string(),
    );
    let started = Instant::now();
    let mut t = Tally::default();
    let mut skipped = 0;
    let token = "x".repeat(48);
    let batches = pack(&stocks, |b| {
        AlpacaProvider::corporate_actions_url(b, &start, &end, Some(&token)).len()
    });
    for batch in batches {
        let refs: Vec<&str> = batch.iter().map(String::as_str).collect();
        let requested: Vec<VenueSymbol> = batch
            .iter()
            .filter_map(|s| VenueSymbol::new(s).ok())
            .collect();
        let mut page: Option<String> = None;
        loop {
            let url = AlpacaProvider::corporate_actions_url(&refs, &start, &end, page.as_deref());
            let mut next = None;
            step!(t, format!("corporate actions {}", batch.join(",")), {
                let f = alpaca::fetch_authenticated(client, credentials, &url).await?;
                let r = ingest_corporate_actions(conn, &raw(&f), &requested).await?;
                t.records += 1;
                t.bytes += f.body.len();
                t.inserted += r.inserted;
                t.replaced += r.replaced;
                t.unchanged += r.unchanged;
                skipped += r.skipped;
                next = alpaca::decode_corporate_actions(&f.body)
                    .map_err(|e| Error::Usage(e.to_string()))?
                    .next_page_token;
                Ok::<(), Error>(())
            });
            tokio::time::sleep(Duration::from_millis(350)).await;
            match next {
                Some(p) => page = Some(p),
                None => break,
            }
        }
    }
    t.print(&format!("corporate-actions {start}..{end}"), started);
    if skipped > 0 {
        println!("  ({skipped} actions of unmodelled types skipped)");
    }
    Ok(t.failures)
}

// ---------------------------------------------------------------- event calendars

/// The curated release list (`data/reference/economic-releases.json`).
const ECONOMIC_RELEASES: &str = "data/reference/economic-releases.json";

async fn economic(conn: &mut PgConnection, client: &HttpClient) -> Result<usize, Error> {
    let Some(key) = std::env::var("FRED_API_KEY").ok().filter(|k| !k.is_empty()) else {
        println!("history economic: skipped (FRED_API_KEY not set)");
        return Ok(0);
    };
    let text =
        std::fs::read(ECONOMIC_RELEASES).map_err(|e| Error::Io(ECONOMIC_RELEASES.into(), e))?;
    let list: serde_json::Value =
        serde_json::from_slice(&text).map_err(|e| Error::Json(ECONOMIC_RELEASES.into(), e))?;
    let today = Utc::now().date_naive();
    let window = (today - TimeDelta::days(30), today + TimeDelta::days(180));
    let (start, end) = (window.0.to_string(), window.1.to_string());
    let started = Instant::now();
    let mut t = Tally::default();
    for r in list["releases"].as_array().into_iter().flatten() {
        let (Some(id), Some(name), Some(category)) = (
            r["key"].as_str(),
            r["name"].as_str(),
            r["category"].as_str(),
        ) else {
            return Err(Error::Usage(format!(
                "{ECONOMIC_RELEASES}: a release needs key, name and category"
            )));
        };
        step!(t, format!("fred release {id}"), {
            let f = fred::fetch_release_dates(client, &key, id, &start, &end).await?;
            let n =
                ingest_economic_release_dates(conn, &raw(&f), id, name, category, window).await?;
            t.records += 1;
            t.bytes += f.body.len();
            t.inserted += n;
            Ok::<(), Error>(())
        });
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    t.print(&format!("economic {start}..{end}"), started);
    Ok(t.failures)
}

async fn earnings(conn: &mut PgConnection, client: &HttpClient) -> Result<usize, Error> {
    let Some(key) = std::env::var("FINNHUB_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    else {
        println!("history earnings: skipped (FINNHUB_API_KEY not set)");
        return Ok(0);
    };
    let (stocks, _) = symbols(conn, alpaca::SOURCE_ID, true).await?;
    let requested: Vec<VenueSymbol> = stocks
        .iter()
        .filter_map(|s| VenueSymbol::new(s).ok())
        .collect();
    let today = Utc::now().date_naive();
    let started = Instant::now();
    let mut t = Tally::default();
    // Two-week windows from a month back to three months ahead.
    let mut from = today - TimeDelta::days(30);
    let last = today + TimeDelta::days(90);
    while from <= last {
        let to = (from + TimeDelta::days(13)).min(last);
        step!(t, format!("finnhub earnings {from}..{to}"), {
            let f =
                finnhub::fetch_earnings(client, &key, &from.to_string(), &to.to_string()).await?;
            let r = ingest_earnings(conn, &raw(&f), &requested).await?;
            t.records += 1;
            t.bytes += f.body.len();
            t.inserted += r.inserted;
            t.replaced += r.replaced;
            t.unchanged += r.unchanged;
            Ok::<(), Error>(())
        });
        tokio::time::sleep(Duration::from_millis(1100)).await;
        from = to + TimeDelta::days(1);
    }
    t.print("earnings", started);
    Ok(t.failures)
}

// ---------------------------------------------------------------- reference

fn months_back(today: NaiveDate, n: u32) -> Vec<(i32, u32)> {
    let mut out = Vec::new();
    let (mut y, mut m) = (today.year(), today.month());
    for _ in 0..n {
        out.push((y, m));
        if m == 1 {
            y -= 1;
            m = 12;
        } else {
            m -= 1;
        }
    }
    out
}

async fn reference(
    conn: &mut PgConnection,
    client: &HttpClient,
    eia_key: Option<&str>,
) -> Result<usize, Error> {
    let started = Instant::now();
    let mut t = Tally::default();
    let today = Utc::now().date_naive();
    let gap = || tokio::time::sleep(Duration::from_millis(500));

    step!(t, "ecb 90d", {
        let f = ecb::fetch_hist_90d(client).await?;
        let r =
            ingest_history_for(conn, &EcbProvider::new(), &EcbNormalizer, &raw(&f), None).await?;
        t.quotes(&f, &r);
        Ok::<(), Error>(())
    });
    gap().await;

    let (series, _) = symbols(conn, bank_of_canada::SOURCE_ID, false).await?;
    let refs: Vec<&str> = series.iter().map(String::as_str).collect();
    if !refs.is_empty() {
        step!(t, "bank-of-canada", {
            let url = BankOfCanadaProvider::recent_url(&refs, 90);
            let f = bank_of_canada::fetch_url(client, &url).await?;
            let r = ingest_history_for(
                conn,
                &BankOfCanadaProvider::new(),
                &BankOfCanadaNormalizer,
                &raw(&f),
                None,
            )
            .await?;
            t.quotes(&f, &r);
            Ok::<(), Error>(())
        });
        gap().await;
    }

    step!(t, "fed-h10", {
        let f = fed_h10::fetch_package_last(client, 90).await?;
        let r = ingest_history_for(
            conn,
            &FedH10Provider::new(),
            &FedH10Normalizer,
            &raw(&f),
            None,
        )
        .await?;
        t.quotes(&f, &r);
        Ok::<(), Error>(())
    });
    gap().await;

    let (series, _) = symbols(conn, bank_indonesia::SOURCE_ID, false).await?;
    let (start, end) = (today - TimeDelta::days(90), today);
    for s in &series {
        let Some(url) = BankIndonesiaProvider::rates_url(s, &start.to_string(), &end.to_string())
        else {
            continue;
        };
        step!(t, format!("bank-indonesia {s}"), {
            let f = bank_indonesia::fetch_rates(client, &url).await?;
            let r = ingest_history_for(
                conn,
                &BankIndonesiaProvider::new(),
                &BankIndonesiaNormalizer,
                &raw(&f),
                Some(&sym(s)),
            )
            .await?;
            t.quotes(&f, &r);
            Ok::<(), Error>(())
        });
        gap().await;
    }

    let (currencies, _) = symbols(conn, bnm::SOURCE_ID, false).await?;
    for c in &currencies {
        for (y, m) in months_back(today, 3) {
            step!(t, format!("bnm {c} {y}-{m:02}"), {
                let f = bnm::fetch_month(client, &bnm::month_url(c, y, m)).await?;
                let r = ingest_history_for(
                    conn,
                    &BnmMonthProvider::new(),
                    &BnmMonthNormalizer,
                    &raw(&f),
                    Some(&sym(c)),
                )
                .await?;
                t.quotes(&f, &r);
                Ok::<(), Error>(())
            });
            gap().await;
        }
    }

    let (cbm_symbols, _) = symbols(conn, cbm::SOURCE_ID, false).await?;
    if !cbm_symbols.is_empty() {
        let requested: Vec<VenueSymbol> = cbm_symbols
            .iter()
            .filter_map(|s| VenueSymbol::new(s).ok())
            .collect();
        for back in 1..=60 {
            let day = today - TimeDelta::days(back);
            if matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
                continue;
            }
            step!(t, format!("cbm {day}"), {
                let f = cbm::fetch_history_day(client, &day.format("%d-%m-%Y").to_string()).await?;
                let r = ingest_quotes_for(
                    conn,
                    &CbmProvider::new(),
                    &CbmNormalizer,
                    &raw(&f),
                    Some(&requested),
                )
                .await?;
                t.quotes(&f, &r);
                Ok::<(), Error>(())
            });
            gap().await;
        }
    }

    match eia_key {
        None => println!("history reference eia: skipped (EIA_API_KEY not set)"),
        Some(key) => {
            let (series, _) = symbols(conn, eia::SOURCE_ID, false).await?;
            let mut routes: Vec<(&str, Vec<&str>)> = Vec::new();
            for s in &series {
                let Some(route) = eia::route_of(s) else {
                    continue;
                };
                match routes.iter_mut().find(|(r, _)| *r == route) {
                    Some((_, v)) => v.push(s),
                    None => routes.push((route, vec![s])),
                }
            }
            for (route, list) in routes {
                step!(t, format!("eia {route}"), {
                    let f = eia::fetch_series_days(client, key, route, &list, 90).await?;
                    let r = ingest_history_for(
                        conn,
                        &EiaProvider::new(),
                        &EiaNormalizer,
                        &raw(&f),
                        None,
                    )
                    .await?;
                    t.quotes(&f, &r);
                    Ok::<(), Error>(())
                });
                gap().await;
            }
        }
    }

    step!(t, "worldbank", {
        let f = worldbank::fetch_monthly_workbook(client).await?;
        let r = ingest_history_for(
            conn,
            &WorldBankProvider::new(),
            &WorldBankNormalizer,
            &raw(&f),
            None,
        )
        .await?;
        t.quotes(&f, &r);
        Ok::<(), Error>(())
    });

    t.print("reference", started);
    Ok(t.failures)
}

/// V1.9 stablecoin/fiat venues (docs/v1.9-live-fx.md): each venue's 1h and
/// 1d bars for its declared markets, one market and interval per request
/// (OKX pages back 100 bars at a time). Returns the failed requests.
async fn stablecoin_venue_bars(
    conn: &mut PgConnection,
    client: &HttpClient,
    o: &Options,
) -> Result<usize, Error> {
    let now = Utc::now();
    let mut failures = 0;
    let windows = [
        (BarInterval::OneHour, o.days_1h),
        (BarInterval::OneDay, o.days_1d),
    ];
    let start = |days: i64| now - TimeDelta::days(days);

    // Binance-layout klines (Binance, Coins.ph, HashKey): up to 1,000 bars
    // from the window's start, which covers 30 days of hours or a year of days.
    for source in [binance::SOURCE_ID, coins_ph::SOURCE_ID, hashkey::SOURCE_ID] {
        let (markets, _) = symbols(conn, source, true).await?;
        let started = Instant::now();
        let mut t = Tally::default();
        for market in &markets {
            for (interval, days) in windows {
                let tf = if interval == BarInterval::OneHour {
                    "1h"
                } else {
                    "1d"
                };
                let from = start(days).timestamp_millis();
                step!(t, format!("{source} {market} {tf}"), {
                    let normalizer = KlineBarNormalizer(interval);
                    let (f, r) = match source {
                        binance::SOURCE_ID => {
                            let f = binance::fetch_klines(client, market, tf, from, 1000).await?;
                            let r = ingest_bars(
                                conn,
                                &BinanceProvider::new(),
                                &normalizer,
                                &raw(&f),
                                &sym(market),
                            )
                            .await?;
                            (f, r)
                        }
                        coins_ph::SOURCE_ID => {
                            let f = coins_ph::fetch_klines(client, market, tf, from, 1000).await?;
                            let r = ingest_bars(
                                conn,
                                &CoinsPhProvider::new(),
                                &normalizer,
                                &raw(&f),
                                &sym(market),
                            )
                            .await?;
                            (f, r)
                        }
                        _ => {
                            let f = hashkey::fetch_klines(client, market, tf, from, 1000).await?;
                            let r = ingest_bars(
                                conn,
                                &HashKeyProvider::new(),
                                &normalizer,
                                &raw(&f),
                                &sym(market),
                            )
                            .await?;
                            (f, r)
                        }
                    };
                    t.bars(&f, &r);
                    Ok::<(), Error>(())
                });
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
        t.print(&format!("bars {source}"), started);
        failures += t.failures;
    }

    // OKX: newest first, 100 bars per page, paged back with `after`.
    let (markets, _) = symbols(conn, okx::SOURCE_ID, true).await?;
    let started = Instant::now();
    let mut t = Tally::default();
    for market in &markets {
        for (interval, days) in windows {
            let bar = if interval == BarInterval::OneHour {
                "1H"
            } else {
                "1Dutc"
            };
            let oldest = start(days).timestamp_millis();
            let mut after: Option<i64> = None;
            loop {
                let mut next: Option<i64> = None;
                step!(t, format!("okx {market} {bar}"), {
                    let f = okx::fetch_candles(client, market, bar, after).await?;
                    let r = ingest_bars(
                        conn,
                        &OkxProvider::new(),
                        &OkxBarNormalizer(interval),
                        &raw(&f),
                        &sym(market),
                    )
                    .await?;
                    let page = <OkxProvider as undrly_provider::BarsProvider>::decode_bars(
                        &OkxProvider::new(),
                        &f.body,
                    )
                    .map_err(undrly_ingest::IngestError::from)?;
                    next = page.0.last().map(|c| c.ts).filter(|ts| *ts > oldest);
                    t.bars(&f, &r);
                    Ok::<(), Error>(())
                });
                tokio::time::sleep(Duration::from_millis(250)).await;
                match next {
                    Some(ts) => after = Some(ts),
                    None => break,
                }
            }
        }
    }
    t.print("bars okx", started);
    failures += t.failures;

    // Indodax and Bitkub chart history: one window per request.
    let to = now.timestamp();
    for source in [indodax::SOURCE_ID, bitkub::SOURCE_ID] {
        let (markets, _) = symbols(conn, source, true).await?;
        let started = Instant::now();
        let mut t = Tally::default();
        for market in &markets {
            for (interval, days) in windows {
                let tf = if interval == BarInterval::OneHour {
                    "60"
                } else {
                    "1D"
                };
                let from = start(days).timestamp();
                step!(t, format!("{source} {market} {tf}"), {
                    let (f, r) = if source == indodax::SOURCE_ID {
                        let f = indodax::fetch_history(client, market, tf, from, to).await?;
                        let r = ingest_bars(
                            conn,
                            &IndodaxProvider::new(),
                            &IndodaxBarNormalizer(interval),
                            &raw(&f),
                            &sym(market),
                        )
                        .await?;
                        (f, r)
                    } else {
                        let f = bitkub::fetch_history(client, market, tf, from, to).await?;
                        let r = ingest_bars(
                            conn,
                            &BitkubProvider::new(),
                            &BitkubBarNormalizer(interval),
                            &raw(&f),
                            &sym(market),
                        )
                        .await?;
                        (f, r)
                    };
                    t.bars(&f, &r);
                    Ok::<(), Error>(())
                });
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        t.print(&format!("bars {source}"), started);
        failures += t.failures;
    }
    Ok(failures)
}
