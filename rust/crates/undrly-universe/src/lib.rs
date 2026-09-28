//! V1.1 universe builder (docs/v1.1-universe.md §2–§3).
//!
//! ```text
//! raw universe files (manifest) + id map + V1 curated universe
//!   → build (pure) → snapshot (curated format) + updated id map + report
//! ```
//!
//! The same inputs give byte-identical outputs: everything is sorted, no
//! wall-clock time is read (only sources' own dates and recorded fetch
//! times), and new canonical ids come only from the caller's `mint` for keys
//! the id map does not yet have.
//!
//! Identity rules:
//!
//! - crypto assets are keyed by CoinGecko id; venue markets map to them only
//!   through CoinGecko's exchange tickers, confirmed against the venue's own
//!   market list;
//! - perpetuals are keyed by Hyperliquid market name; the underlying comes
//!   only from CoinGecko's Hyperliquid derivatives tickers;
//! - equities are keyed by a **build-local** `spy:<CUSIP>` key. The CUSIP is
//!   never emitted as an identifier and no ISIN is constructed from it; the
//!   issuer entity carries the CIK from SEC's ticker table.
//!
//! Anything that cannot be mapped is excluded and listed with its reason.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use undrly_core::{
    CanonicalId, Category, ChainId, CurrencyId, DeploymentId, EntityId, InstrumentId, ListingId,
    VenueId,
};
use undrly_provider::QuoteProvider;
use undrly_provider::curated::{
    AliasRecord, EntityRecord, InstrumentRecord, ListingRecord, QuoteAggregationRecord,
    QuoteFeedRecord, Universe, UniverseMemberRecord, UniverseRecord, VenueRecord,
};
use undrly_provider::hyperliquid::HyperliquidProvider;
use undrly_provider::{coinbase, coingecko, kraken, nasdaq, sec, ssga};

pub mod cusip;
pub mod fx;

/// Source id under which built snapshots are stored.
pub const SNAPSHOT_SOURCE: &str = "undrly-universe";

/// Feed freshness of market-data feeds (V1 default).
const MARKET_STALE_AFTER: u32 = 300;

/// Paths of the raw files inside the raw directory.
pub mod paths {
    pub const MARKETS: &str = "coingecko/markets.json";
    pub const COINS_LIST: &str = "coingecko/coins-list.json";
    pub const KRAKEN_TICKERS: &str = "coingecko/kraken-tickers-";
    pub const COINBASE_TICKERS: &str = "coingecko/gdax-tickers-";
    pub const HYPERLIQUID_DERIVATIVES: &str = "coingecko/hyperliquid-derivatives.json";
    pub const KRAKEN_ASSET_PAIRS: &str = "kraken/asset-pairs.json";
    pub const COINBASE_PRODUCTS: &str = "coinbase/products.json";
    pub const HYPERLIQUID_META: &str = "hyperliquid/meta-and-asset-ctxs.json";
    pub const SPY_HOLDINGS: &str = "ssga/holdings-daily-us-en-spy.xlsx";
    pub const NASDAQ100: &str = "nasdaq/nasdaq100.json";
    pub const SEC_TICKERS: &str = "sec-edgar/company_tickers_exchange.json";
}

/// What `universe fetch` downloaded: one entry per raw file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestFile {
    /// Relative to the raw directory, e.g. `coingecko/markets.json`.
    pub path: String,
    pub source: String,
    /// The upstream record key (the URL; `POST <url> <body>` for POSTs).
    pub record_key: String,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub bytes: usize,
    /// RFC 3339 receipt time.
    pub fetched_at: String,
}

/// Build key → canonical id. The only stateful build input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdMap {
    pub version: u32,
    pub ids: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("missing raw file `{0}` (run `universe fetch`)")]
    Missing(String),
    #[error("`{path}`: SHA-256 does not match the manifest")]
    Hash { path: String },
    #[error("{0}")]
    Decode(#[from] undrly_provider::DecodeError),
    #[error("id map: {0}")]
    IdMap(String),
    #[error("V1 universe: {0}")]
    V1(String),
    #[error("{0}")]
    Invalid(String),
}

/// Build inputs. `files` maps manifest paths to their bytes.
pub struct Inputs<'a> {
    pub manifest: &'a Manifest,
    pub files: &'a BTreeMap<String, Vec<u8>>,
    /// The curated V1 universe (pinned ids reused, never redeclared).
    pub v1: &'a Universe,
    pub ids: IdMap,
}

pub struct Build {
    pub snapshot: Universe,
    pub ids: IdMap,
    /// Markdown import report.
    pub report: String,
    pub coverage: Coverage,
}

/// Headline numbers (also in the report).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub instruments_new: usize,
    pub entities_new: usize,
    pub feeds_new: usize,
    pub members: BTreeMap<String, usize>,
    pub excluded: BTreeMap<String, usize>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A new id of `category`.
pub fn mint(category: Category) -> CanonicalId {
    match category {
        Category::Entity => EntityId::generate().canonical(),
        Category::Instrument => InstrumentId::generate().canonical(),
        Category::Listing => ListingId::generate().canonical(),
        Category::Venue => VenueId::generate().canonical(),
        Category::Currency => CurrencyId::generate().canonical(),
        Category::Chain => ChainId::generate().canonical(),
        Category::Deployment => DeploymentId::generate().canonical(),
    }
}

struct V1 {
    by_key: HashMap<String, String>,
    ids: HashSet<String>,
    /// V1's feeds: (source, symbol, price type).
    feeds: HashSet<(String, String, String)>,
}

impl V1 {
    fn new(u: &Universe) -> Self {
        let mut by_key = HashMap::new();
        for (k, id) in u
            .currencies
            .iter()
            .map(|x| (&x.key, &x.id))
            .chain(u.entities.iter().map(|x| (&x.key, &x.id)))
            .chain(u.venues.iter().map(|x| (&x.key, &x.id)))
            .chain(u.instruments.iter().map(|x| (&x.key, &x.id)))
            .chain(u.listings.iter().map(|x| (&x.key, &x.id)))
        {
            by_key.insert(k.clone(), id.clone());
        }
        let ids = by_key.values().cloned().collect();
        let feeds = u
            .quote_feeds
            .iter()
            .map(|f| (f.source.clone(), f.symbol.clone(), f.price_type.clone()))
            .collect();
        Self { by_key, ids, feeds }
    }

    fn id(&self, key: &str) -> Result<String, BuildError> {
        self.by_key
            .get(key)
            .cloned()
            .ok_or_else(|| BuildError::V1(format!("no object with key `{key}`")))
    }
}

/// V1 objects the build reuses, by build key. Hand-pinned: the equity and
/// issuer pins are NVIDIA's SPY CUSIP key and CIK, stated here explicitly,
/// not derived from V1's ISIN.
const V1_PINS: [(&str, &str); 10] = [
    ("coingecko:bitcoin", "btc"),
    ("coingecko:usd-coin", "usdc"),
    ("coingecko:tether", "usdt"),
    ("hyperliquid:BTC", "btc-perp"),
    ("spy:67066G104", "nvda"),
    ("sec:1045810", "nvidia"),
    ("venue:XNAS", "nasdaq"),
    ("listing:spy:67066G104:XNAS", "nvda-xnas"),
    ("venue:kraken", "kraken"),
    ("venue:coinbase", "coinbase"),
];

#[derive(Default)]
struct Out {
    entities: BTreeMap<String, EntityRecord>,
    venues: BTreeMap<String, VenueRecord>,
    instruments: BTreeMap<String, InstrumentRecord>,
    listings: BTreeMap<String, ListingRecord>,
    relationships: BTreeSet<(String, String, String)>,
    aliases: BTreeSet<(String, String, String)>,
    feeds: BTreeMap<(String, String, String), QuoteFeedRecord>,
    aggregations: BTreeMap<(String, String), QuoteAggregationRecord>,
    universes: Vec<UniverseRecord>,
    /// (section, subject, reason)
    excluded: BTreeSet<(String, String, String)>,
    notes: Vec<String>,
}

struct Ctx<'a> {
    inputs: &'a Inputs<'a>,
    v1: V1,
    ids: IdMap,
    mint: &'a mut dyn FnMut(Category) -> CanonicalId,
    minted: usize,
    out: Out,
}

impl Ctx<'_> {
    fn file(&self, path: &str) -> Result<(&ManifestFile, &[u8]), BuildError> {
        let entry = self
            .inputs
            .manifest
            .files
            .iter()
            .find(|f| f.path == path)
            .ok_or_else(|| BuildError::Missing(path.to_owned()))?;
        let bytes = self
            .inputs
            .files
            .get(path)
            .ok_or_else(|| BuildError::Missing(path.to_owned()))?;
        if sha256_hex(bytes) != entry.sha256 {
            return Err(BuildError::Hash {
                path: path.to_owned(),
            });
        }
        Ok((entry, bytes))
    }

    fn pages(&self, prefix: &str) -> Result<Vec<&[u8]>, BuildError> {
        let mut paths: Vec<&str> = self
            .inputs
            .manifest
            .files
            .iter()
            .filter(|f| f.path.starts_with(prefix))
            .map(|f| f.path.as_str())
            .collect();
        paths.sort_unstable();
        if paths.is_empty() {
            return Err(BuildError::Missing(format!("{prefix}*")));
        }
        paths.into_iter().map(|p| Ok(self.file(p)?.1)).collect()
    }

    /// The canonical id of `key`, minting and recording one if new.
    fn id(&mut self, key: &str, category: Category) -> Result<String, BuildError> {
        if let Some(id) = self.ids.ids.get(key) {
            let parsed =
                CanonicalId::parse(id).map_err(|e| BuildError::IdMap(format!("{key}: {e}")))?;
            if parsed.category() != category {
                return Err(BuildError::IdMap(format!("{key} is not a {category}")));
            }
            return Ok(id.clone());
        }
        let id = (self.mint)(category).to_string();
        self.minted += 1;
        self.ids.ids.insert(key.to_owned(), id.clone());
        Ok(id)
    }

    fn is_v1(&self, id: &str) -> bool {
        self.v1.ids.contains(id)
    }

    /// How the snapshot refers to `key`: V1 objects by canonical id (they are
    /// not redeclared), new objects by their build key.
    fn node_ref(&self, key: &str) -> String {
        match self.ids.ids.get(key) {
            Some(id) if self.is_v1(id) => id.clone(),
            _ => key.to_owned(),
        }
    }

    fn exclude(&mut self, section: &str, subject: impl Into<String>, reason: impl Into<String>) {
        self.out
            .excluded
            .insert((section.to_owned(), subject.into(), reason.into()));
    }

    fn alias(&mut self, node: &str, text: &str, kind: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.out
                .aliases
                .insert((node.to_owned(), text.to_owned(), kind.to_owned()));
        }
    }

    fn feed(&mut self, feed: QuoteFeedRecord) {
        let key = (
            feed.source.clone(),
            feed.symbol.clone(),
            feed.price_type.clone(),
        );
        if self.v1.feeds.contains(&key) {
            return;
        }
        self.out.feeds.insert(key, feed);
    }

    fn universe(
        &mut self,
        key: &str,
        path: &str,
        as_of: String,
        members: Vec<UniverseMemberRecord>,
    ) -> Result<(), BuildError> {
        let (entry, _) = self.file(path)?;
        let record = UniverseRecord {
            key: key.to_owned(),
            source: entry.source.clone(),
            record_key: entry.record_key.clone(),
            sha256: entry.sha256.clone(),
            as_of,
            members,
        };
        self.out.universes.push(record);
        Ok(())
    }
}

/// Builds the snapshot. `mint` supplies ids for new keys (normally [`mint`]).
pub fn build(
    inputs: &Inputs<'_>,
    mint: &mut dyn FnMut(Category) -> CanonicalId,
) -> Result<Build, BuildError> {
    let v1 = V1::new(inputs.v1);
    let mut ids = inputs.ids.clone();
    ids.version = 1;
    for (key, v1_key) in V1_PINS {
        let id = v1.id(v1_key)?;
        match ids.ids.get(key) {
            Some(existing) if *existing != id => {
                return Err(BuildError::IdMap(format!(
                    "{key} is pinned to V1 `{v1_key}` ({id}), map has {existing}"
                )));
            }
            _ => {
                ids.ids.insert(key.to_owned(), id);
            }
        }
    }
    let mut ctx = Ctx {
        inputs,
        v1,
        ids,
        mint,
        minted: 0,
        out: Out::default(),
    };
    crypto(&mut ctx)?;
    perps(&mut ctx)?;
    equities(&mut ctx)?;
    finish(ctx)
}

fn usd(ctx: &Ctx<'_>) -> Result<String, BuildError> {
    ctx.v1.id("usd")
}

// ---------------------------------------------------------------- crypto

/// The CoinGecko assets the build creates, id → (symbol, name).
fn crypto_asset(
    ctx: &mut Ctx<'_>,
    coin: &str,
    symbol: &str,
    name: &str,
) -> Result<String, BuildError> {
    let key = format!("coingecko:{coin}");
    let id = ctx.id(&key, Category::Instrument)?;
    if !ctx.is_v1(&id) && !ctx.out.instruments.contains_key(&key) {
        ctx.out.instruments.insert(
            key.clone(),
            InstrumentRecord {
                key: key.clone(),
                id,
                class: "crypto_asset".into(),
                name: name.trim().to_owned(),
                isin: None,
                figi: None,
                contract_multiplier: None,
                unit_of_measure: None,
                base: None,
                quote: None,
            },
        );
        ctx.alias(&key, &symbol.to_uppercase(), "symbol");
        ctx.alias(&key, name, "name");
    }
    Ok(ctx.node_ref(&key))
}

/// Venue USD markets per CoinGecko coin id: coin → pair symbols, via
/// CoinGecko's tickers and confirmed in the venue's own market list.
fn venue_markets(
    ctx: &mut Ctx<'_>,
    section: &str,
    tickers: &[coingecko::Ticker],
    venue_symbol: &dyn Fn(&coingecko::Ticker) -> Option<String>,
    wanted: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in tickers {
        let Some(coin) = t.coin_id.as_deref() else {
            continue;
        };
        if t.target != "USD" || !wanted.contains(coin) {
            continue;
        }
        match venue_symbol(t) {
            Some(symbol) => {
                found.entry(coin.to_owned()).or_default().insert(symbol);
            }
            None => ctx.exclude(
                section,
                format!("{coin} ({}/{})", t.base, t.target),
                "CoinGecko lists the market but the venue's market list has no online match",
            ),
        }
    }
    let mut out = BTreeMap::new();
    for (coin, symbols) in found {
        if symbols.len() == 1 {
            out.insert(coin, symbols.into_iter().next().expect("one"));
        } else {
            ctx.exclude(
                section,
                coin,
                format!("several USD markets {symbols:?}; none chosen"),
            );
        }
    }
    out
}

fn crypto(ctx: &mut Ctx<'_>) -> Result<(), BuildError> {
    let markets = coingecko::decode_markets(ctx.file(paths::MARKETS)?.1)?;
    let mut kraken_tickers = Vec::new();
    for page in ctx.pages(paths::KRAKEN_TICKERS)? {
        kraken_tickers.extend(coingecko::decode_tickers(page)?.tickers);
    }
    let mut coinbase_tickers = Vec::new();
    for page in ctx.pages(paths::COINBASE_TICKERS)? {
        coinbase_tickers.extend(coingecko::decode_tickers(page)?.tickers);
    }
    let pairs = kraken::decode_asset_pairs(ctx.file(paths::KRAKEN_ASSET_PAIRS)?.1)?;
    let products = coinbase::decode_products(ctx.file(paths::COINBASE_PRODUCTS)?.1)?;

    let mut members = Vec::new();
    let mut wanted = BTreeSet::new();
    let mut as_of: Option<String> = None;
    let mut rows: Vec<&coingecko::Market> = markets.iter().collect();
    rows.sort_by_key(|m| (m.market_cap_rank.unwrap_or(u32::MAX), m.id.clone()));
    for m in rows {
        let node = crypto_asset(ctx, &m.id, &m.symbol, &m.name)?;
        wanted.insert(m.id.clone());
        members.push(UniverseMemberRecord {
            node,
            rank: m.market_cap_rank,
            source_symbol: Some(m.symbol.clone()),
        });
        if let Some(t) = &m.last_updated
            && as_of.as_ref().is_none_or(|a| t > a)
        {
            as_of = Some(t.clone());
        }
    }
    let as_of =
        as_of.ok_or_else(|| BuildError::Invalid("coins/markets has no last_updated".into()))?;

    let by_wsname: HashMap<&str, &str> = pairs
        .result
        .iter()
        .filter(|(_, p)| p.status.as_deref() == Some("online"))
        .filter_map(|(k, p)| Some((p.wsname.as_deref()?, k.as_str())))
        .collect();
    let kraken = venue_markets(
        ctx,
        "crypto-top100 / Kraken",
        &kraken_tickers,
        &|t| {
            by_wsname
                .get(format!("{}/{}", t.base, t.target).as_str())
                .map(|k| (*k).to_owned())
        },
        &wanted,
    );
    let coinbase = venue_markets(
        ctx,
        "crypto-top100 / Coinbase",
        &coinbase_tickers,
        &|t| {
            products
                .iter()
                .find(|p| {
                    p.base_currency == t.base
                        && p.quote_currency == t.target
                        && p.status == "online"
                        && !p.trading_disabled
                })
                .map(|p| p.id.clone())
        },
        &wanted,
    );

    let usd = usd(ctx)?;
    let kraken_venue = ctx.v1.id("kraken")?;
    let coinbase_venue = ctx.v1.id("coinbase")?;
    let mut both = 0;
    let mut kraken_only = 0;
    let mut coinbase_only = 0;
    for coin in &wanted {
        let node = ctx.node_ref(&format!("coingecko:{coin}"));
        let k = kraken.get(coin);
        let c = coinbase.get(coin);
        if let Some(symbol) = k {
            ctx.feed(QuoteFeedRecord {
                source: kraken::SOURCE_ID.into(),
                symbol: symbol.clone(),
                subject: node.clone(),
                unit: usd.clone(),
                basis: "venue".into(),
                venue: Some(kraken_venue.clone()),
                price_type: "last".into(),
                stale_after_seconds: Some(MARKET_STALE_AFTER),
                freshness_clock: None,
                inverted: false,
            });
            ctx.out
                .relationships
                .insert((node.clone(), "TRADES_ON".into(), kraken_venue.clone()));
        }
        if let Some(symbol) = c {
            ctx.feed(QuoteFeedRecord {
                source: coinbase::SOURCE_ID.into(),
                symbol: symbol.clone(),
                subject: node.clone(),
                unit: usd.clone(),
                basis: "venue".into(),
                venue: Some(coinbase_venue.clone()),
                price_type: "mid".into(),
                stale_after_seconds: Some(MARKET_STALE_AFTER),
                freshness_clock: None,
                inverted: false,
            });
            ctx.out.relationships.insert((
                node.clone(),
                "TRADES_ON".into(),
                coinbase_venue.clone(),
            ));
        }
        match (k, c) {
            (Some(_), Some(_)) => {
                both += 1;
                // BTC/USD's declaration is V1's.
                if !ctx.is_v1(&node) {
                    ctx.out.aggregations.insert(
                        (node.clone(), usd.clone()),
                        QuoteAggregationRecord {
                            subject: node.clone(),
                            unit: usd.clone(),
                            method: "mean-venue-mid-v1".into(),
                        },
                    );
                }
            }
            (Some(_), None) => kraken_only += 1,
            (None, Some(_)) => coinbase_only += 1,
            (None, None) => ctx.exclude(
                "crypto-top100 / quotes",
                coin.clone(),
                "no crosswalked Kraken or Coinbase USD market: imported without a quote feed",
            ),
        }
    }
    ctx.out.notes.push(format!(
        "crypto-top100: {} members; USD markets on Kraken + Coinbase {both}, Kraken only {kraken_only}, Coinbase only {coinbase_only}, none {}",
        members.len(),
        wanted.len() - both - kraken_only - coinbase_only
    ));
    ctx.universe("crypto-top100", paths::MARKETS, as_of, members)
}

// ---------------------------------------------------------------- perps

/// Hyperliquid's contract specification (read 2026-09-28):
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/trading/contract-specifications>
/// "USDC margining, USDT denominated linear contracts. That is, the oracle
/// price is denominated in USDT, but the collateral is USDC. [...] these
/// contracts are technically quanto contracts where USDT pnl is denominated
/// in USDC." and "Currently, the only USDC-denominated perpetual contracts are
/// PURR-USD and HYPE-USD". Market names of those exceptions on the first
/// perp dex; every other main-dex perpetual is USDT-denominated.
pub const HYPERLIQUID_USDC_DENOMINATED: [&str; 2] = ["HYPE", "PURR"];

/// The spot token the documentation names as the first perp dex's
/// collateral (USDC is spot token 0). If `meta.collateralToken` says
/// otherwise, no margin or settlement edge is asserted.
pub const HYPERLIQUID_USDC_TOKEN: u32 = 0;

/// Hyperliquid's `k` prefix denotes a 1,000-unit contract (`kPEPE`).
pub fn contract_multiplier(name: &str) -> Option<&'static str> {
    let rest = name.strip_prefix('k')?;
    (!rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
    .then_some("1000")
}

fn perps(ctx: &mut Ctx<'_>) -> Result<(), BuildError> {
    let (meta_entry, meta_bytes) = ctx.file(paths::HYPERLIQUID_META)?;
    let as_of = meta_entry.fetched_at.clone();
    let meta = HyperliquidProvider::new().decode_quote(meta_bytes)?;
    let derivatives = coingecko::decode_derivatives(ctx.file(paths::HYPERLIQUID_DERIVATIVES)?.1)?;
    let coins = coingecko::decode_coins_list(ctx.file(paths::COINS_LIST)?.1)?;
    let coins: HashMap<&str, &coingecko::Coin> = coins.iter().map(|c| (c.id.as_str(), c)).collect();
    // Main-market name (upper case) → CoinGecko coin ids.
    let mut underlying: HashMap<String, BTreeSet<String>> = HashMap::new();
    for t in &derivatives.tickers {
        let Some(coin) = t.coin_id.as_deref() else {
            continue;
        };
        if t.base.contains(':')
            || t.contract_type.as_deref() != Some("perpetual")
            || t.target != "USD"
        {
            continue;
        }
        underlying
            .entry(t.base.to_uppercase())
            .or_default()
            .insert(coin.to_owned());
    }

    let usdc = ctx.v1.id("usdc")?;
    let usdt = ctx.v1.id("usdt")?;
    let venue = ctx.v1.id("hyperliquid")?;
    // Margin and cash flows (profit and loss, funding) are in the dex's
    // collateral; only assert USDC when the response confirms the token.
    let collateral = (meta.collateral_token == Some(HYPERLIQUID_USDC_TOKEN)).then(|| usdc.clone());
    if collateral.is_none() {
        ctx.exclude(
            "hyperliquid-perps / collateral",
            "meta",
            format!(
                "collateralToken is {:?}, not the documented USDC token: no MARGINED_IN or SETTLES_IN edges",
                meta.collateral_token
            ),
        );
    }
    let mut live: Vec<&str> = meta
        .universe
        .iter()
        .filter(|a| !a.is_delisted)
        .map(|a| a.name.as_str())
        .collect();
    live.sort_unstable();
    live.dedup();
    let delisted = meta.universe.iter().filter(|a| a.is_delisted).count();
    let mut members = Vec::new();
    let (mut with_edge, mut multiplied) = (0, 0);
    for name in live {
        let key = format!("hyperliquid:{name}");
        let id = ctx.id(&key, Category::Instrument)?;
        let node = ctx.node_ref(&key);
        let multiplier = contract_multiplier(name);
        if multiplier.is_some() {
            multiplied += 1;
        }
        // The price unit: what the oracle and mark are denominated in.
        let denomination = if HYPERLIQUID_USDC_DENOMINATED.contains(&name) {
            usdc.clone()
        } else {
            usdt.clone()
        };
        if !ctx.is_v1(&id) {
            let base = name
                .strip_prefix('k')
                .filter(|_| multiplier.is_some())
                .unwrap_or(name);
            ctx.out.instruments.insert(
                key.clone(),
                InstrumentRecord {
                    key: key.clone(),
                    id,
                    class: "perpetual_future".into(),
                    name: match multiplier {
                        Some(m) => {
                            format!("{name} Perpetual (Hyperliquid, {m} {base} per contract)")
                        }
                        None => format!("{name} Perpetual (Hyperliquid)"),
                    },
                    isin: None,
                    figi: None,
                    contract_multiplier: multiplier.map(str::to_owned),
                    unit_of_measure: None,
                    base: None,
                    quote: None,
                },
            );
            ctx.alias(&key, &format!("{name}-PERP"), "symbol");
            ctx.alias(&key, &format!("{name} perpetual"), "name");
            ctx.out.relationships.insert((
                key.clone(),
                "DENOMINATED_IN".into(),
                denomination.clone(),
            ));
            if let Some(collateral) = &collateral {
                ctx.out.relationships.insert((
                    key.clone(),
                    "SETTLES_IN".into(),
                    collateral.clone(),
                ));
                ctx.out.relationships.insert((
                    key.clone(),
                    "MARGINED_IN".into(),
                    collateral.clone(),
                ));
            }
            ctx.out
                .relationships
                .insert((key.clone(), "TRADES_ON".into(), venue.clone()));
            match underlying.get(&name.to_uppercase()) {
                Some(set) if set.len() == 1 => {
                    let coin = set.iter().next().expect("one");
                    match coins.get(coin.as_str()) {
                        Some(c) => {
                            let under = crypto_asset(ctx, coin, &c.symbol, &c.name)?;
                            ctx.out.relationships.insert((
                                key.clone(),
                                "DERIVES_FROM".into(),
                                under,
                            ));
                            with_edge += 1;
                        }
                        None => ctx.exclude(
                            "hyperliquid-perps / underlying",
                            name,
                            format!(
                                "CoinGecko id `{coin}` is not in coins/list: no DERIVES_FROM edge"
                            ),
                        ),
                    }
                }
                Some(set) => ctx.exclude(
                    "hyperliquid-perps / underlying",
                    name,
                    format!("several CoinGecko ids {set:?}: no DERIVES_FROM edge"),
                ),
                None => ctx.exclude(
                    "hyperliquid-perps / underlying",
                    name,
                    "no CoinGecko derivatives crosswalk: no DERIVES_FROM edge",
                ),
            }
        } else {
            with_edge += 1; // V1's BTC-PERP carries its edge.
        }
        ctx.feed(QuoteFeedRecord {
            source: "hyperliquid".into(),
            symbol: name.to_owned(),
            subject: node.clone(),
            unit: denomination.clone(),
            basis: "venue".into(),
            venue: Some(venue.clone()),
            price_type: "mark".into(),
            stale_after_seconds: Some(MARKET_STALE_AFTER),
            freshness_clock: None,
            inverted: false,
        });
        members.push(UniverseMemberRecord {
            node,
            rank: None,
            source_symbol: Some(name.to_owned()),
        });
    }
    ctx.out.notes.push(format!(
        "hyperliquid-perps: {} live perps ({delisted} delisted skipped); {with_edge} with an underlying edge; {multiplied} with contract multiplier 1000",
        members.len()
    ));
    ctx.universe("hyperliquid-perps", paths::HYPERLIQUID_META, as_of, members)
}

// ---------------------------------------------------------------- equities

/// SEC exchange name → MIC.
fn mic_of(exchange: &str) -> Option<&'static str> {
    match exchange {
        "NYSE" => Some("XNYS"),
        "Nasdaq" => Some("XNAS"),
        "CBOE" => Some("BATS"),
        _ => None,
    }
}

/// Common venue names users type in `VENUE:SYMBOL` queries, as symbol aliases
/// of the venue whose MIC stays its identifier. Explicit, never inferred.
/// `CBOE` is deliberately absent: it is also Cboe Global Markets' ticker, so
/// it would make the bare symbol ambiguous; `BATS:…` (the MIC) works.
const VENUE_ALIASES: [(&str, &str); 1] = [("XNYS", "NYSE")];

fn venue_name(mic: &str) -> &'static str {
    match mic {
        "XNYS" => "New York Stock Exchange",
        "BATS" => "Cboe BZX Exchange",
        _ => "Nasdaq",
    }
}

/// `As of 23-Sep-2026` → `2026-09-23T00:00:00Z`.
fn spy_date(text: &str) -> Result<String, BuildError> {
    let date = text.trim().trim_start_matches("As of").trim();
    chrono::NaiveDate::parse_from_str(date, "%d-%b-%Y")
        .map(|d| format!("{}T00:00:00Z", d.format("%Y-%m-%d")))
        .map_err(|e| BuildError::Invalid(format!("SPY holdings date `{text}`: {e}")))
}

/// `Sep 24, 2026` → `2026-09-24T00:00:00Z`.
fn nasdaq_date(text: &str) -> Result<String, BuildError> {
    chrono::NaiveDate::parse_from_str(text.trim(), "%b %d, %Y")
        .map(|d| format!("{}T00:00:00Z", d.format("%Y-%m-%d")))
        .map_err(|e| BuildError::Invalid(format!("Nasdaq list date `{text}`: {e}")))
}

fn equities(ctx: &mut Ctx<'_>) -> Result<(), BuildError> {
    let holdings = ssga::decode_holdings(ctx.file(paths::SPY_HOLDINGS)?.1)?;
    let sec_rows = sec::decode_company_tickers_exchange(ctx.file(paths::SEC_TICKERS)?.1)?;
    let mut by_ticker: HashMap<&str, Vec<&sec::TickerExchange>> = HashMap::new();
    for r in &sec_rows {
        by_ticker.entry(r.ticker.as_str()).or_default().push(r);
    }
    let usd = usd(ctx)?;
    let iex = ctx.v1.id("iex")?;
    let section = "sp500";
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    // (listing symbol on XNAS) → node, for Nasdaq-100 matching.
    let mut on_nasdaq: HashMap<String, String> = HashMap::new();
    let mut per_mic: BTreeMap<&str, usize> = BTreeMap::new();
    for h in &holdings.rows {
        let label = h
            .ticker
            .clone()
            .or_else(|| h.name.clone())
            .unwrap_or_else(|| format!("row {}", h.row));
        let Some(ticker) = h.ticker.as_deref().filter(|t| *t != "-") else {
            ctx.exclude(section, label, "no ticker (cash or other non-security row)");
            continue;
        };
        let Some(cusip) = h.identifier.as_deref().filter(|c| cusip::is_valid(c)) else {
            ctx.exclude(
                section,
                label,
                format!(
                    "identifier {:?} is not a valid CUSIP (not a security row)",
                    h.identifier
                ),
            );
            continue;
        };
        if h.local_currency.as_deref() != Some("USD") {
            ctx.exclude(
                section,
                label,
                format!("local currency {:?}", h.local_currency),
            );
            continue;
        }
        if !seen.insert(cusip.to_owned()) {
            ctx.exclude(section, label, "duplicate row for the same security");
            continue;
        }
        // One explicit class-share rule: SPY/Alpaca `BRK.B` is SEC `BRK-B`.
        let sec_ticker = ticker.replace('.', "-");
        let sec_row = match by_ticker.get(sec_ticker.as_str()).map(Vec::as_slice) {
            Some([one]) => *one,
            Some(many) => {
                ctx.exclude(
                    section,
                    label,
                    format!("SEC lists {} issuers for ticker {sec_ticker}", many.len()),
                );
                continue;
            }
            None => {
                ctx.exclude(
                    section,
                    label,
                    format!("ticker {sec_ticker} not in SEC's ticker table"),
                );
                continue;
            }
        };
        let Some(mic) = sec_row.exchange.as_deref().and_then(mic_of) else {
            ctx.exclude(
                section,
                label,
                format!("SEC exchange {:?} has no MIC rule", sec_row.exchange),
            );
            continue;
        };
        *per_mic.entry(mic).or_default() += 1;

        let key = format!("spy:{cusip}");
        let id = ctx.id(&key, Category::Instrument)?;
        let node = ctx.node_ref(&key);
        if !ctx.is_v1(&id) {
            let name = h.name.clone().unwrap_or_else(|| ticker.to_owned());
            // Issuer: one entity per CIK.
            let entity_key = format!("sec:{}", sec_row.cik);
            let entity_id = ctx.id(&entity_key, Category::Entity)?;
            let entity = ctx.node_ref(&entity_key);
            if !ctx.is_v1(&entity_id) && !ctx.out.entities.contains_key(&entity_key) {
                ctx.out.entities.insert(
                    entity_key.clone(),
                    EntityRecord {
                        key: entity_key.clone(),
                        id: entity_id,
                        kind: "company".into(),
                        name: sec_row.name.trim().to_owned(),
                        lei: None,
                        cik: Some(format!("{:010}", sec_row.cik)),
                    },
                );
                ctx.alias(&entity_key, &sec_row.name, "name");
            }
            ctx.out.instruments.insert(
                key.clone(),
                InstrumentRecord {
                    key: key.clone(),
                    id,
                    class: "equity".into(),
                    name: name.clone(),
                    isin: None,
                    figi: None,
                    contract_multiplier: None,
                    unit_of_measure: None,
                    base: None,
                    quote: None,
                },
            );
            ctx.alias(&key, ticker, "symbol");
            ctx.alias(&key, &name, "name");
            ctx.out
                .relationships
                .insert((key.clone(), "ISSUED_BY".into(), entity));
            ctx.out
                .relationships
                .insert((key.clone(), "DENOMINATED_IN".into(), usd.clone()));
            ctx.out
                .relationships
                .insert((key.clone(), "TRADES_ON".into(), iex.clone()));
            let venue_key = format!("venue:{mic}");
            let venue_id = ctx.id(&venue_key, Category::Venue)?;
            let venue = ctx.node_ref(&venue_key);
            if !ctx.is_v1(&venue_id) && !ctx.out.venues.contains_key(&venue_key) {
                ctx.out.venues.insert(
                    venue_key.clone(),
                    VenueRecord {
                        key: venue_key.clone(),
                        id: venue_id,
                        name: venue_name(mic).into(),
                        mic: Some(mic.into()),
                    },
                );
                ctx.alias(&venue_key, mic, "symbol");
                ctx.alias(&venue_key, venue_name(mic), "name");
                for (_, alias) in VENUE_ALIASES.iter().filter(|(m, _)| *m == mic) {
                    ctx.alias(&venue_key, alias, "symbol");
                }
            }
            let listing_key = format!("listing:{key}:{mic}");
            let listing_id = ctx.id(&listing_key, Category::Listing)?;
            ctx.out.listings.insert(
                listing_key.clone(),
                ListingRecord {
                    key: listing_key,
                    id: listing_id,
                    instrument: key.clone(),
                    venue,
                    symbol: ticker.to_owned(),
                    figi: None,
                },
            );
            // IEX's last trade and IEX's top-of-book mid: two feeds.
            for price_type in ["last", "mid"] {
                ctx.feed(QuoteFeedRecord {
                    source: "alpaca".into(),
                    symbol: ticker.to_owned(),
                    subject: key.clone(),
                    unit: usd.clone(),
                    basis: "venue".into(),
                    venue: Some(iex.clone()),
                    price_type: price_type.into(),
                    stale_after_seconds: Some(MARKET_STALE_AFTER),
                    freshness_clock: None,
                    inverted: false,
                });
            }
        }
        if mic == "XNAS" {
            on_nasdaq.insert(ticker.to_owned(), node.clone());
        }
        members.push(UniverseMemberRecord {
            node,
            rank: None,
            source_symbol: Some(ticker.to_owned()),
        });
    }
    ctx.out.notes.push(format!(
        "sp500 (SPY holdings, {}): {} holdings rows, {} imported (primary listing {}), {} other rows (notes/footer) ignored",
        holdings.as_of,
        holdings.rows.len(),
        members.len(),
        per_mic
            .iter()
            .map(|(m, n)| format!("{m} {n}"))
            .collect::<Vec<_>>()
            .join(" · "),
        holdings.other_rows
    ));
    let as_of = spy_date(&holdings.as_of)?;
    ctx.universe("sp500", paths::SPY_HOLDINGS, as_of, members)?;

    // Nasdaq-100: only members that are imported S&P 500 securities listed
    // on Nasdaq under the same symbol (decision 4). The list is optional.
    if !ctx
        .inputs
        .manifest
        .files
        .iter()
        .any(|f| f.path == paths::NASDAQ100)
    {
        ctx.out
            .notes
            .push("nasdaq100: not built. Universe support is modeled, but live membership import is deferred pending an approved machine-readable source.".into());
        return Ok(());
    }
    let list = nasdaq::decode_nasdaq100(ctx.file(paths::NASDAQ100)?.1)?;
    let mut members = Vec::new();
    let mut rows: Vec<&nasdaq::Member> = list.data.data.rows.iter().collect();
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    for m in rows {
        match on_nasdaq.get(&m.symbol) {
            Some(node) => members.push(UniverseMemberRecord {
                node: node.clone(),
                rank: None,
                source_symbol: Some(m.symbol.clone()),
            }),
            None => ctx.exclude(
                "nasdaq100",
                format!("{} ({})", m.symbol, m.company_name),
                "not an imported S&P 500 security listed on Nasdaq: no trustworthy identifier, skipped",
            ),
        }
    }
    ctx.out.notes.push(format!(
        "nasdaq100: {} listed, {} members imported",
        list.data.data.rows.len(),
        members.len()
    ));
    let as_of = nasdaq_date(
        list.data
            .date
            .as_deref()
            .ok_or_else(|| BuildError::Invalid("Nasdaq list has no date".into()))?,
    )?;
    ctx.universe("nasdaq100", paths::NASDAQ100, as_of, members)
}

// ---------------------------------------------------------------- output

fn finish(ctx: Ctx<'_>) -> Result<Build, BuildError> {
    let Ctx {
        inputs,
        v1,
        ids,
        minted,
        out,
        ..
    } = ctx;
    let mut universes = out.universes;
    universes.sort_by(|a, b| a.key.cmp(&b.key));

    // Symbol aliases shared by several nodes (V1 + snapshot).
    let mut by_symbol: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for a in &inputs.v1.aliases {
        if a.kind == "symbol" {
            let node = v1
                .by_key
                .get(&a.node)
                .cloned()
                .unwrap_or_else(|| a.node.clone());
            by_symbol
                .entry(a.alias.to_uppercase())
                .or_default()
                .insert(node);
        }
    }
    for (node, text, kind) in &out.aliases {
        if kind == "symbol" {
            let id = ids.ids.get(node).cloned().unwrap_or_else(|| node.clone());
            by_symbol.entry(text.to_uppercase()).or_default().insert(id);
        }
    }
    let collisions: Vec<(&String, usize)> = by_symbol
        .iter()
        .filter(|(_, nodes)| nodes.len() > 1)
        .map(|(s, n)| (s, n.len()))
        .collect();

    let snapshot = Universe {
        dataset: "undrly-universe-snapshot".into(),
        version: 1,
        description: "Generated by `undrly-collect universe build` from local raw universe files (docs/v1.1-universe.md). Local/private use only; not for redistribution.".into(),
        currencies: Vec::new(),
        entities: out.entities.into_values().collect(),
        venues: out.venues.into_values().collect(),
        instruments: out.instruments.into_values().collect(),
        listings: out.listings.into_values().collect(),
        relationships: out.relationships.into_iter().collect(),
        aliases: out
            .aliases
            .into_iter()
            .map(|(node, alias, kind)| AliasRecord { node, alias, kind })
            .collect(),
        quote_feeds: out.feeds.into_values().collect(),
        quote_aggregations: out.aggregations.into_values().collect(),
        universes,
    };

    let mut coverage = Coverage {
        instruments_new: snapshot.instruments.len(),
        entities_new: snapshot.entities.len(),
        feeds_new: snapshot.quote_feeds.len(),
        ..Default::default()
    };
    for u in &snapshot.universes {
        coverage.members.insert(u.key.clone(), u.members.len());
    }
    for (section, _, _) in &out.excluded {
        *coverage.excluded.entry(section.clone()).or_default() += 1;
    }

    let mut r = String::new();
    let _ = writeln!(r, "# Universe import report\n");
    let _ = writeln!(
        r,
        "Generated by `undrly-collect universe build`. Local/private use only (docs/v1.1-universe.md §12).\n"
    );
    let _ = writeln!(r, "## Universes\n");
    let _ = writeln!(
        r,
        "| Universe | As of | Source | Record | SHA-256 | Members |"
    );
    let _ = writeln!(r, "| --- | --- | --- | --- | --- | --- |");
    for u in &snapshot.universes {
        let _ = writeln!(
            r,
            "| {} | {} | {} | `{}` | `{}…` | {} |",
            u.key,
            u.as_of,
            u.source,
            u.record_key,
            &u.sha256[..16],
            u.members.len()
        );
    }
    let _ = writeln!(r, "\n## Summary\n");
    for n in &out.notes {
        let _ = writeln!(r, "- {n}");
    }
    let feeds_by_source = snapshot
        .quote_feeds
        .iter()
        .fold(BTreeMap::new(), |mut m, f| {
            *m.entry(f.source.as_str()).or_insert(0usize) += 1;
            m
        });
    let _ = writeln!(
        r,
        "- new objects: {} instruments, {} issuer entities, {} venues, {} listings, {} relationships, {} aliases",
        snapshot.instruments.len(),
        snapshot.entities.len(),
        snapshot.venues.len(),
        snapshot.listings.len(),
        snapshot.relationships.len(),
        snapshot.aliases.len()
    );
    let _ = writeln!(
        r,
        "- new quote feeds: {} ({}); aggregation declarations: {}",
        snapshot.quote_feeds.len(),
        feeds_by_source
            .iter()
            .map(|(s, n)| format!("{s} {n}"))
            .collect::<Vec<_>>()
            .join(", "),
        snapshot.quote_aggregations.len()
    );
    let _ = writeln!(
        r,
        "- ids minted this build: {minted}; id map entries: {}",
        ids.ids.len()
    );
    let _ = writeln!(
        r,
        "- identifiers: no ISIN or CUSIP is emitted for SPY securities (never constructed); issuer entities carry SEC CIKs"
    );
    let _ = writeln!(r, "\n## Symbol collisions\n");
    let _ = writeln!(
        r,
        "{} symbols name more than one node. Use a pair (`ETH/USD`), a venue symbol (`NASDAQ:AAPL`), an identifier or an id to disambiguate.\n",
        collisions.len()
    );
    for (s, n) in &collisions {
        let _ = writeln!(r, "- `{s}`: {n} nodes");
    }
    let _ = writeln!(r, "\n## Exclusions\n");
    let mut current = "";
    for (section, subject, reason) in &out.excluded {
        if section != current {
            let _ = writeln!(r, "\n### {section} ({})\n", coverage.excluded[section]);
            current = section;
        }
        let _ = writeln!(r, "- {subject}: {reason}");
    }
    let _ = writeln!(r, "\n## Inputs\n");
    let _ = writeln!(r, "| File | Source | Bytes | SHA-256 | Fetched |");
    let _ = writeln!(r, "| --- | --- | --- | --- | --- |");
    let mut files: Vec<&ManifestFile> = inputs.manifest.files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for f in files {
        let _ = writeln!(
            r,
            "| `{}` | {} | {} | `{}…` | {} |",
            f.path,
            f.source,
            f.bytes,
            &f.sha256[..16],
            f.fetched_at
        );
    }
    Ok(Build {
        snapshot,
        ids,
        report: r,
        coverage,
    })
}

/// Pretty JSON with a trailing newline (the byte format of every output).
pub fn to_json<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = serde_json::to_vec_pretty(value).expect("serializable");
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests;
