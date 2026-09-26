//! Federal Reserve Board H.10 foreign exchange rates, through the Data
//! Download Program (CSV). No authentication.
//!
//! Noon buying rates in New York for cable transfers payable in foreign
//! currencies, certified by the Federal Reserve Bank of New York: official
//! reference rates, not market quotes (no bid/ask). The H.10 release is
//! **weekly** (Mondays), with one value per business day of the previous
//! week, so the newest value is typically 3 to 10 days old. `ND` marks a
//! day without a rate (e.g. a holiday).
//!
//! Series `RXI_N.B.XX` are units of foreign currency per 1 USD (`USD/JPY`);
//! series `RXI$US_N.B.XX` are USD per 1 unit of foreign currency (`NZD/USD`).
//! Values are kept as their exact text.

pub const SOURCE_ID: &str = "fed-h10";
/// The Data Download Program's preformatted H.10 package (every currency,
/// business days), last 10 observations, series in columns.
pub const PACKAGE_URL: &str = "https://www.federalreserve.gov/datadownload/Output.aspx?rel=H10&series=60f32914ab61dfab590e0e470153e3ae&lastobs=10&from=&to=&filetype=csv&label=include&layout=seriescolumn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H10 {
    /// Series names (`RXI_N.B.JA`), in column order.
    pub series: Vec<String>,
    /// Multiplier per series (`1` for every H.10 series).
    pub multipliers: Vec<String>,
    /// `(YYYY-MM-DD, one value per series)`, values verbatim (`ND` = none).
    pub rows: Vec<(String, Vec<String>)>,
}

/// One CSV line: comma-separated, fields optionally double-quoted (no
/// escaped quotes occur in the H.10 package).
fn fields(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        let (field, tail) = if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').ok_or("unterminated quote")?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            (&rest[..end], &rest[end..])
        };
        out.push(field.to_owned());
        match tail.strip_prefix(',') {
            Some(t) => rest = t,
            None if tail.is_empty() => return Ok(out),
            None => return Err(format!("unexpected text after a field: `{tail}`")),
        }
    }
}

fn decode(payload: &[u8]) -> Result<H10, String> {
    let text = std::str::from_utf8(payload).map_err(|e| e.to_string())?;
    let mut series = None;
    let mut multipliers = None;
    let mut rows = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut f = fields(line.trim_end_matches('\r'))?;
        if f.is_empty() {
            continue;
        }
        let head = f.remove(0);
        match head.as_str() {
            "Time Period" => series = Some(f),
            "Multiplier:" => multipliers = Some(f),
            h if h.len() == 10 && h.as_bytes()[4] == b'-' && h.as_bytes()[7] == b'-' => {
                rows.push((head, f));
            }
            _ => {}
        }
    }
    let series: Vec<String> = series.ok_or("no `Time Period` header")?;
    let multipliers: Vec<String> = multipliers.ok_or("no `Multiplier:` row")?;
    if multipliers.len() != series.len() || rows.iter().any(|(_, v)| v.len() != series.len()) {
        return Err("rows and header differ in width".into());
    }
    Ok(H10 {
        series,
        multipliers,
        rows,
    })
}

crate::quote_provider!(FedH10Provider, H10, decode);

/// The package URL with the last `n` observations (history backfill).
pub fn package_url(n: u32) -> String {
    PACKAGE_URL.replace("lastobs=10", &format!("lastobs={n}"))
}

/// Fetches the H.10 package with the last `n` observations (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_package_last(
    client: &crate::http::HttpClient,
    n: u32,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get_accepting(&package_url(n), "text/csv", &[]).await
}

/// Fetches the H.10 package (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_package(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get_accepting(PACKAGE_URL, "text/csv", &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_package_verbatim() {
        let h = FedH10Provider::new()
            .decode_quote(&crate::fixture("fed-h10/h10-lastobs10.csv"))
            .unwrap();
        assert_eq!(h.series.len(), 23);
        let ja = h.series.iter().position(|s| s == "RXI_N.B.JA").unwrap();
        let nz = h.series.iter().position(|s| s == "RXI$US_N.B.NZ").unwrap();
        let (date, values) = h.rows.last().unwrap();
        assert_eq!(date, "2026-09-18");
        assert_eq!(values[ja], "156.8700");
        assert_eq!(values[nz], "0.5710");
        assert!(h.rows.iter().any(|(_, v)| v[ja] == "ND"));
        assert!(h.multipliers.iter().all(|m| m == "1"));
    }

    #[test]
    fn parses_quoted_and_bare_fields() {
        assert_eq!(
            fields(r#""a","b c",1,ND"#).unwrap(),
            vec!["a", "b c", "1", "ND"]
        );
        assert!(fields(r#""a"x"#).is_err());
        assert!(
            FedH10Provider::new()
                .decode_quote(b"2026-09-18,1\n")
                .is_err()
        );
    }
}
