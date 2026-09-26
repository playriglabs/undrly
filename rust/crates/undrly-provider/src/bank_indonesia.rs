//! Bank Indonesia exchange-rate web service (`wskursbi.asmx`, XML over HTTP
//! GET). No authentication.
//!
//! Two rate series, both official reference rates, not market quotes:
//!
//! - **JISDOR** (`getSubKursJisdor3`, rows `*_subkursasing`): the Jakarta
//!   Interbank Spot Dollar Rate, IDR per 1 USD, set by Bank Indonesia each
//!   business day (about 10:00 WIB) from interbank transactions. Buy and sell
//!   columns carry the same single rate.
//! - **Kurs transaksi BI** (`getSubKursLokal3`, rows `*_subkurslokal`): Bank
//!   Indonesia's transaction (buying/selling) rates in IDR per `nil` units of
//!   a currency. Their middle rate (`(beli + jual) / 2`, "kurs tengah") is the
//!   reference; the buy/sell rates are Bank Indonesia's own, not a market
//!   spread, and are never exposed as a bid/ask.
//!
//! Each row states its date with a +07:00 offset. Values are kept verbatim.

use quick_xml::Reader;
use quick_xml::events::Event;

pub const SOURCE_ID: &str = "bank-indonesia";
pub const BASE_URL: &str = "https://www.bi.go.id/biwebservice/wskursbi.asmx";

/// Which Bank Indonesia series a response holds (from its row elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Series {
    Jisdor,
    Transaction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// ISO 4217 code (the payload pads it with spaces; trimmed here).
    pub currency: String,
    /// Units of the currency the rates are for (`1.00`, `100.00` for JPY).
    pub units: String,
    pub buy: String,
    pub sell: String,
    /// RFC 3339 date-time with offset, as stated.
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rates {
    pub series: Series,
    pub rows: Vec<Row>,
}

fn decode(payload: &[u8]) -> Result<Rates, String> {
    let mut reader = Reader::from_reader(payload);
    let mut series = None;
    let mut rows = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut current: Option<String> = None;
    let mut in_table = false;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == "Table" {
                    in_table = true;
                    fields.clear();
                } else if in_table {
                    current = Some(name);
                }
            }
            Event::Text(t) if in_table => {
                if let Some(name) = &current {
                    let text = t.unescape().map_err(|e| e.to_string())?.into_owned();
                    fields.push((name.clone(), text));
                }
            }
            Event::End(e) => {
                let name = e.local_name();
                if name.as_ref() == b"Table" {
                    in_table = false;
                    let (row, kind) = row(&fields)?;
                    if series.replace(kind).is_some_and(|k| k != kind) {
                        return Err("mixed series in one response".into());
                    }
                    rows.push(row);
                } else {
                    current = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(Rates {
        series: series.ok_or("no rate rows")?,
        rows,
    })
}

fn row(fields: &[(String, String)]) -> Result<(Row, Series), String> {
    let kind = if fields.iter().any(|(n, _)| n.ends_with("_subkursasing")) {
        Series::Jisdor
    } else if fields.iter().any(|(n, _)| n.ends_with("_subkurslokal")) {
        Series::Transaction
    } else {
        return Err("unknown row element names".into());
    };
    let get = |prefix: &str| {
        fields
            .iter()
            .find(|(n, _)| n.starts_with(prefix))
            .map(|(_, v)| v.trim().to_owned())
            .ok_or_else(|| format!("row without `{prefix}…`"))
    };
    Ok((
        Row {
            currency: get("mts_")?,
            units: get("nil_")?,
            buy: get("beli_")?,
            sell: get("jual_")?,
            date: get("tgl_")?,
        },
        kind,
    ))
}

crate::quote_provider!(BankIndonesiaProvider, Rates, decode);

impl BankIndonesiaProvider {
    /// JISDOR (`JISDOR-USD`) or a transaction rate (`KURS-SGD`) between two
    /// dates (`YYYY-MM-DD`, inclusive). `None` for another symbol.
    pub fn rates_url(symbol: &str, start: &str, end: &str) -> Option<String> {
        let (series, currency) = symbol.split_once('-')?;
        let valid = currency.len() == 3 && currency.bytes().all(|b| b.is_ascii_uppercase());
        match series {
            "JISDOR" if valid => Some(format!(
                "{BASE_URL}/getSubKursJisdor3?mts={currency}&startDate={start}&endDate={end}"
            )),
            "KURS" if valid => Some(format!(
                "{BASE_URL}/getSubKursLokal3?mts={currency}&startdate={start}&enddate={end}"
            )),
            _ => None,
        }
    }
}

/// Fetches one series between two dates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_rates(
    client: &crate::http::HttpClient,
    url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get_accepting(url, "application/xml, text/xml", &[])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_jisdor_and_transaction_rates() {
        let j = BankIndonesiaProvider::new()
            .decode_quote(&crate::fixture("bank-indonesia/jisdor-usd.xml"))
            .unwrap();
        assert_eq!(j.series, Series::Jisdor);
        assert_eq!(j.rows.len(), 10);
        assert_eq!(
            j.rows[0],
            Row {
                currency: "USD".into(),
                units: "1.00".into(),
                buy: "17917.00".into(),
                sell: "17917.00".into(),
                date: "2026-09-25T00:00:00+07:00".into(),
            }
        );
        let k = BankIndonesiaProvider::new()
            .decode_quote(&crate::fixture("bank-indonesia/kurs-sgd.xml"))
            .unwrap();
        assert_eq!(k.series, Series::Transaction);
        assert_eq!(k.rows[0].currency, "SGD");
        assert_eq!(
            (k.rows[0].buy.as_str(), k.rows[0].sell.as_str()),
            ("13921.60", "14062.61")
        );
    }

    #[test]
    fn builds_urls_for_known_series_only() {
        assert_eq!(
            BankIndonesiaProvider::rates_url("JISDOR-USD", "2026-09-12", "2026-09-26").unwrap(),
            "https://www.bi.go.id/biwebservice/wskursbi.asmx/getSubKursJisdor3?mts=USD&startDate=2026-09-12&endDate=2026-09-26"
        );
        assert!(BankIndonesiaProvider::rates_url("KURS-MYR", "a", "b").is_some());
        assert!(BankIndonesiaProvider::rates_url("KURS-myr", "a", "b").is_none());
        assert!(BankIndonesiaProvider::rates_url("USD", "a", "b").is_none());
        assert!(BankIndonesiaProvider::new().decode_quote(b"<x/>").is_err());
    }
}
