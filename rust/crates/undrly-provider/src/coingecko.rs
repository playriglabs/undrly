//! CoinGecko public API: the crypto universe and the venue crosswalk.
//!
//! CoinGecko is **not** a quote source for Undrly. It is used at universe
//! build time only:
//!
//! - `coins/markets`: the top 500 by market capitalisation, two pages of
//!   250 (the universe; the top 100 and 250 are its first ranks);
//! - `coins/list`: id → symbol and name for any coin id (perp underlyings);
//! - `exchanges/{id}/tickers`: which CoinGecko coin each venue market trades,
//!   as the venue spells it (`XBT`/`USD` on Kraken). This is the crosswalk
//!   from venue symbols to assets; symbols are never matched by name.
//! - `derivatives/exchanges/{id}`: the same for perpetual venues.
//!
//! Prices in these payloads are ignored.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "coingecko";
pub const API: &str = "https://api.coingecko.com/api/v3";
/// Tickers per page of `exchanges/{id}/tickers`.
pub const TICKERS_PER_PAGE: usize = 100;

/// Rows per `coins/markets` page (CoinGecko's maximum).
pub const MARKETS_PER_PAGE: usize = 250;

pub fn markets_url(page: u32) -> String {
    format!("{API}/coins/markets?vs_currency=usd&order=market_cap_desc&per_page=250&page={page}")
}

pub fn coins_list_url() -> String {
    format!("{API}/coins/list")
}

pub fn exchange_tickers_url(exchange: &str, page: u32) -> String {
    format!("{API}/exchanges/{exchange}/tickers?page={page}")
}

pub fn derivatives_url(exchange: &str) -> String {
    format!("{API}/derivatives/exchanges/{exchange}?include_tickers=unexpired")
}

/// One `coins/markets` row. Only identity fields are decoded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Market {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub market_cap_rank: Option<u32>,
    /// RFC 3339; the newest one is the snapshot's as-of time.
    pub last_updated: Option<String>,
}

/// One `coins/list` row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Coin {
    pub id: String,
    pub symbol: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tickers {
    pub tickers: Vec<Ticker>,
}

/// A venue market as CoinGecko maps it: the venue's own base/target
/// spelling and the CoinGecko ids they trade.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ticker {
    pub base: String,
    pub target: String,
    pub coin_id: Option<String>,
    pub target_coin_id: Option<String>,
    #[serde(default)]
    pub is_stale: bool,
    #[serde(default)]
    pub is_anomaly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Derivatives {
    pub tickers: Vec<DerivativeTicker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DerivativeTicker {
    pub symbol: String,
    pub base: String,
    pub target: String,
    pub coin_id: Option<String>,
    pub contract_type: Option<String>,
}

fn reject(reason: impl ToString) -> DecodeError {
    DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: reason.to_string(),
    }
}

fn json<'a, T: Deserialize<'a>>(what: &str, payload: &'a [u8]) -> Result<T, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| reject(format!("{what}: {e}")))
}

pub fn decode_markets(payload: &[u8]) -> Result<Vec<Market>, DecodeError> {
    json("coins/markets", payload)
}

pub fn decode_coins_list(payload: &[u8]) -> Result<Vec<Coin>, DecodeError> {
    json("coins/list", payload)
}

pub fn decode_tickers(payload: &[u8]) -> Result<Tickers, DecodeError> {
    json("exchange tickers", payload)
}

pub fn decode_derivatives(payload: &[u8]) -> Result<Derivatives, DecodeError> {
    json("derivatives exchange", payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_identity_fields_and_ignores_prices() {
        let m = decode_markets(
            br#"[{"id":"bitcoin","symbol":"btc","name":"Bitcoin","market_cap_rank":1,
                  "current_price":64000.12,"last_updated":"2026-09-25T09:12:10.000Z"}]"#,
        )
        .unwrap();
        assert_eq!(m[0].id, "bitcoin");
        assert_eq!(m[0].market_cap_rank, Some(1));
        let t = decode_tickers(
            br#"{"name":"Kraken","tickers":[{"base":"XBT","target":"USD","coin_id":"bitcoin",
                 "target_coin_id":null,"last":1.5,"is_stale":false,"is_anomaly":false}]}"#,
        )
        .unwrap();
        assert_eq!(t.tickers[0].base, "XBT");
        assert_eq!(t.tickers[0].coin_id.as_deref(), Some("bitcoin"));
        let d = decode_derivatives(
            br#"{"name":"Hyperliquid","tickers":[{"symbol":"KPEPE-USD","base":"KPEPE","target":"USD",
                 "coin_id":"pepe","contract_type":"perpetual","last":0.1}]}"#,
        )
        .unwrap();
        assert_eq!(d.tickers[0].coin_id.as_deref(), Some("pepe"));
        assert!(decode_markets(b"{}").is_err());
    }
}
