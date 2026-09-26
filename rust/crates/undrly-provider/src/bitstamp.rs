//! Bitstamp public ticker (`GET /api/v2/ticker/{market}/`), for the fiat
//! markets it lists (EUR/USD, GBP/USD). No authentication.
//!
//! Bitstamp is an execution venue: its best bid and ask are its own order
//! book. The response does not name its market, so one request covers
//! exactly one market (as Coinbase's book). `timestamp` is the ticker's own
//! time in Unix seconds. Values are decimal strings, kept verbatim.

use serde::Deserialize;

pub const SOURCE_ID: &str = "bitstamp";
pub const BASE_URL: &str = "https://www.bitstamp.net/api/v2/ticker";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ticker {
    /// Unix seconds, as text.
    pub timestamp: String,
    pub bid: String,
    pub ask: String,
    pub last: String,
}

fn decode(payload: &[u8]) -> Result<Ticker, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(BitstampProvider, Ticker, decode);

impl BitstampProvider {
    pub fn ticker_url(market: &str) -> String {
        format!("{BASE_URL}/{market}/")
    }
}

/// Fetches the ticker of `market` (e.g. `eurusd`; feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ticker(
    client: &crate::http::HttpClient,
    market: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&BitstampProvider::ticker_url(market), &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_ticker_verbatim() {
        let t = BitstampProvider::new()
            .decode_quote(&crate::fixture("bitstamp/ticker-eurusd.json"))
            .unwrap();
        assert_eq!(t.bid, "1.13882");
        assert_eq!(t.ask, "1.13883");
        assert_eq!(t.timestamp, "1790393400");
        assert_eq!(
            BitstampProvider::ticker_url("eurusd"),
            "https://www.bitstamp.net/api/v2/ticker/eurusd/"
        );
        assert!(
            BitstampProvider::new()
                .decode_quote(b"{\"bid\":1}")
                .is_err()
        );
    }
}
