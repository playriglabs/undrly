//! European Central Bank euro foreign exchange reference rates
//! (`eurofxref-daily.xml`). No authentication.
//!
//! Each rate is units of a currency per 1 EUR, for one date (`<Cube
//! time='YYYY-MM-DD'>`). The ECB sets them around 14:10 CET by concertation
//! between central banks and publishes them around 16:00 CET on TARGET
//! working days, "for information purposes only": reference rates, not
//! market quotes. No bid/ask. Rates are kept as their exact text.

use quick_xml::Reader;
use quick_xml::events::Event;

pub const SOURCE_ID: &str = "ecb";
pub const DAILY_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// The last 90 days of reference rates (same format, one `Cube` per day).
pub const HIST_90D_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml";

/// Reference rates by day: `(ISO 4217 code, rate per 1 EUR)` per date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceRates {
    /// `(YYYY-MM-DD as stated, rates)`, in document order.
    pub days: Vec<(String, Vec<(String, String)>)>,
}

impl ReferenceRates {
    /// The newest day (ISO dates: text order is date order).
    pub fn newest(&self) -> Option<&(String, Vec<(String, String)>)> {
        self.days.iter().max_by(|a, b| a.0.cmp(&b.0))
    }
}

fn decode(payload: &[u8]) -> Result<ReferenceRates, String> {
    let mut reader = Reader::from_reader(payload);
    let mut days: Vec<(String, Vec<(String, String)>)> = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Cube" => {
                let mut currency = None;
                let mut rate = None;
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    let value = a.unescape_value().map_err(|e| e.to_string())?.into_owned();
                    match a.key.as_ref() {
                        b"time" => {
                            if days.iter().any(|(d, _)| *d == value) {
                                return Err(format!("date {value} twice"));
                            }
                            days.push((value, Vec::new()));
                        }
                        b"currency" => currency = Some(value),
                        b"rate" => rate = Some(value),
                        _ => {}
                    }
                }
                match (currency, rate) {
                    (Some(c), Some(r)) => days
                        .last_mut()
                        .ok_or("a rate before any date")?
                        .1
                        .push((c, r)),
                    (None, None) => {}
                    _ => return Err("a rate without a currency, or the reverse".into()),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if days.is_empty() {
        return Err("no reference date".into());
    }
    if days.iter().any(|(_, r)| r.is_empty()) {
        return Err("a date without rates".into());
    }
    Ok(ReferenceRates { days })
}

crate::quote_provider!(EcbProvider, ReferenceRates, decode);

/// Fetches the last 90 days of reference rates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_hist_90d(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get_accepting(HIST_90D_URL, "application/xml, text/xml", &[])
        .await
}

/// Fetches the latest reference rates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_daily(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get_accepting(DAILY_URL, "application/xml, text/xml", &[])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_rates_verbatim() {
        let r = EcbProvider::new()
            .decode_quote(&crate::fixture("ecb/eurofxref-daily.xml"))
            .unwrap();
        let (date, rates) = r.newest().unwrap();
        assert_eq!(date, "2026-09-25");
        assert_eq!(rates.len(), 29);
        assert!(rates.contains(&("USD".into(), "1.1403".into())));
        assert!(rates.contains(&("SEK".into(), "11.2900".into())));
        let two = br#"<Cube><Cube time='2026-09-25'><Cube currency='USD' rate='1.1'/></Cube><Cube time='2026-09-24'><Cube currency='USD' rate='1.2'/></Cube></Cube>"#;
        let h = EcbProvider::new().decode_quote(two).unwrap();
        assert_eq!(h.days.len(), 2);
        assert_eq!(h.newest().unwrap().0, "2026-09-25");
        for bad in [&b"<x/>"[..], b"<Cube time='2026-09-25'></Cube>", b"<Cube"] {
            assert!(EcbProvider::new().decode_quote(bad).is_err());
        }
    }
}
