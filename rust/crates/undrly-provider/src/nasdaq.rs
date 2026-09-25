//! Nasdaq-100 membership from nasdaq.com (`/api/quote/list-type/nasdaq100`):
//! symbols and company names only. Nasdaq gives no ISIN or CUSIP, so a member
//! is imported only when another source identifies the same security
//! (docs/v1.1-universe.md §2).

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "nasdaq";
pub const NASDAQ100_URL: &str = "https://api.nasdaq.com/api/quote/list-type/nasdaq100";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Nasdaq100 {
    pub data: ListData,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListData {
    /// E.g. `Sep 24, 2026`.
    pub date: Option<String>,
    pub data: Rows,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rows {
    pub rows: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub symbol: String,
    pub company_name: String,
}

pub fn decode_nasdaq100(payload: &[u8]) -> Result<Nasdaq100, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_members() {
        let n = decode_nasdaq100(
            br#"{"data":{"date":"Sep 24, 2026","totalrecords":1,"data":{"rows":[
                {"symbol":"EXMP","companyName":"Example Corp Common Stock","marketCap":"1"}]}}}"#,
        )
        .unwrap();
        assert_eq!(n.data.date.as_deref(), Some("Sep 24, 2026"));
        assert_eq!(n.data.data.rows[0].symbol, "EXMP");
        assert!(decode_nasdaq100(br#"{"data":null}"#).is_err());
    }
}
