//! Invesco QQQ ETF holdings (`dng-api.invesco.com`): the Nasdaq-100
//! universe, as a proxy (QQQ tracks the Nasdaq-100 Index), like SPY for the
//! S&P 500 (docs/v1.10-tokenized-stocks.md §3).
//!
//! Each holding states its ticker, CUSIP, security type and currency. The
//! CUSIP is used as a build-local key only; it is never stored or exposed as
//! an identifier, and no ISIN is constructed from it.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "invesco";
pub const QQQ_HOLDINGS_URL: &str = "https://dng-api.invesco.com/cache/v1/accounts/en_US/shareclasses/QQQ/holdings/fund?idType=ticker&interval=monthly&productType=ETF";

/// Security types that are shares of a company: common stock, American
/// depositary receipts, New York registry shares. Cash, futures, currency
/// and collateral rows are not.
pub const EQUITY_TYPES: [&str; 3] = ["COM", "ADR", "DRNY"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holdings {
    /// `YYYY-MM-DD`, the holdings' business date.
    pub effective_business_date: String,
    pub holdings: Vec<Holding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holding {
    pub ticker: Option<String>,
    pub issuer_name: Option<String>,
    pub cusip: Option<String>,
    pub currency: Option<String>,
    pub security_type_code: Option<String>,
}

pub fn decode_holdings(payload: &[u8]) -> Result<Holdings, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_holdings() {
        let h = decode_holdings(
            br#"{"cusip":"QQQ","effectiveDate":"2026-10-03","effectiveBusinessDate":"2026-10-02",
                 "totalNumberOfHoldings":2,"holdings":[
                 {"ticker":"NVDA","issuerName":"NVIDIA Corp","units":1,"cusip":"67066G104",
                  "currency":"USD","securityTypeCode":"COM"},
                 {"ticker":null,"issuerName":null,"cusip":null,"currency":"USD","securityTypeCode":"SYN"}]}"#,
        )
        .unwrap();
        assert_eq!(h.effective_business_date, "2026-10-02");
        assert_eq!(h.holdings[0].cusip.as_deref(), Some("67066G104"));
        assert_eq!(h.holdings[1].ticker, None);
        assert!(decode_holdings(br#"{"holdings":[]}"#).is_err());
    }
}
