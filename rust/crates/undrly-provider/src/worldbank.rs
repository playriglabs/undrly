//! World Bank Commodity Price Data ("Pink Sheet"), monthly workbook
//! (`CMO-Historical-Data-Monthly.xlsx`, sheet `Monthly Prices`).
//!
//! **Monthly averages** in nominal US dollars, published early in the
//! following month: neither spot nor futures prices. Licensed CC BY 4.0.
//!
//! Layout: a preamble (title, `Updated on <date>`), a row of series names
//! (`Maize`, `Wheat, US HRW`, ...), a row of units (`($/mt)`), then one row
//! per month (`2026M08`). Values are returned as stored in the sheet (exact
//! text, e.g. `1.1000000000000001`); missing values are `…`.
//!
//! Each edition has its own URL, linked from the commodity-markets page;
//! [`fetch_monthly_workbook`] discovers it there.

use std::collections::BTreeMap;

use undrly_core::SourceId;

use crate::xlsx::{Cell, read_sheet};
use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "worldbank";
pub const LANDING_URL: &str = "https://www.worldbank.org/en/research/commodity-markets";
pub const WORKBOOK_FILE: &str = "CMO-Historical-Data-Monthly.xlsx";
const WORKBOOK_PREFIX: &str = "https://thedocs.worldbank.org/en/doc/";
pub const SHEET: &str = "Monthly Prices";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyPrices {
    /// E.g. `Updated on September 02, 2026`.
    pub updated: Option<String>,
    /// Column letter → (series name, unit), verbatim (`Maize`, `($/mt)`).
    pub series: BTreeMap<String, (String, String)>,
    /// Months in sheet order.
    pub months: Vec<Month>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Month {
    /// `YYYYMmm`, e.g. `2026M08`.
    pub period: String,
    /// Column letter → stored value.
    pub values: BTreeMap<String, Cell>,
}

impl MonthlyPrices {
    /// The column of the series named exactly `name`.
    pub fn column(&self, name: &str) -> Option<(&str, &str)> {
        self.series
            .iter()
            .find(|(_, (n, _))| n.trim() == name)
            .map(|(c, (_, unit))| (c.as_str(), unit.as_str()))
    }
}

fn is_period(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'M'
        && b[5..].iter().all(u8::is_ascii_digit)
}

pub struct WorldBankProvider {
    source_id: SourceId,
}

impl WorldBankProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }
}

impl Default for WorldBankProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WorldBankProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for WorldBankProvider {
    type Quote = MonthlyPrices;

    fn decode_quote(&self, payload: &[u8]) -> Result<MonthlyPrices, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let rows = read_sheet(payload, SHEET).map_err(reject)?;
        let updated = rows
            .iter()
            .filter_map(|r| r.text("A"))
            .find(|t| t.starts_with("Updated on"))
            .map(str::to_owned);
        let first_month = rows
            .iter()
            .position(|r| r.text("A").is_some_and(is_period))
            .ok_or_else(|| reject("no monthly rows".into()))?;
        if first_month < 2 {
            return Err(reject("no series name and unit rows".into()));
        }
        let (names, units) = (&rows[first_month - 2], &rows[first_month - 1]);
        let series = names
            .cells
            .iter()
            .filter(|(c, _)| c.as_str() != "A")
            .map(|(c, name)| {
                let unit = units.text(c).unwrap_or_default().to_owned();
                (c.clone(), (name.text().to_owned(), unit))
            })
            .collect();
        let months = rows[first_month..]
            .iter()
            .filter_map(|r| {
                let period = r.text("A").filter(|p| is_period(p))?.to_owned();
                let mut values = r.cells.clone();
                values.remove("A");
                Some(Month { period, values })
            })
            .collect();
        Ok(MonthlyPrices {
            updated,
            series,
            months,
        })
    }
}

/// The monthly workbook URL linked from the commodity-markets page.
pub fn workbook_url_in(page: &str) -> Option<String> {
    let end = page.find(WORKBOOK_FILE)? + WORKBOOK_FILE.len();
    let start = page[..end].rfind(WORKBOOK_PREFIX)?;
    let url = &page[start..end];
    url.bytes()
        .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\'' && b != b'<')
        .then(|| url.to_owned())
}

/// Finds the current edition on the landing page, then fetches it (feature
/// `http`). Two requests; only the workbook is returned (and stored).
#[cfg(feature = "http")]
pub async fn fetch_monthly_workbook(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    let page = client.get(LANDING_URL, &[]).await?;
    let url = workbook_url_in(&String::from_utf8_lossy(&page.body)).ok_or_else(|| {
        crate::http::FetchError::Status {
            url: format!("{LANDING_URL} (no {WORKBOOK_FILE} link)"),
            status: 200,
        }
    })?;
    client.get(&url, &[]).await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::xlsx::tests::workbook;

    /// A synthetic workbook in the Pink Sheet layout (values are made up).
    pub(crate) fn synthetic() -> Vec<u8> {
        let s = |r: &str, i: usize| format!(r#"<c r="{r}" t="s"><v>{i}</v></c>"#);
        let n = |r: &str, v: &str| format!(r#"<c r="{r}"><v>{v}</v></c>"#);
        let rows = [
            format!(r#"<row r="4">{}</row>"#, s("A4", 0)),
            format!(r#"<row r="5">{}{}</row>"#, s("B5", 1), s("C5", 2)),
            format!(r#"<row r="6">{}{}</row>"#, s("B6", 3), s("C6", 4)),
            format!(
                r#"<row r="7">{}{}{}</row>"#,
                s("A7", 5),
                n("B7", "224"),
                n("C7", "7.97")
            ),
            format!(
                r#"<row r="8">{}{}{}</row>"#,
                s("A8", 6),
                n("B8", "1.1000000000000001"),
                s("C8", 7)
            ),
        ];
        workbook(
            &[("AFOSHEET", ""), (SHEET, &rows.concat())],
            &[
                "Updated on September 02, 2026",
                "Maize",
                "Coffee, Arabica",
                "($/mt)",
                "($/kg)",
                "2026M07",
                "2026M08",
                "…",
            ],
        )
    }

    #[test]
    fn decodes_series_and_months_verbatim() {
        let p = WorldBankProvider::new().decode_quote(&synthetic()).unwrap();
        assert_eq!(p.updated.as_deref(), Some("Updated on September 02, 2026"));
        assert_eq!(p.column("Maize"), Some(("B", "($/mt)")));
        assert_eq!(p.column("Coffee, Arabica"), Some(("C", "($/kg)")));
        assert_eq!(p.months.len(), 2);
        assert_eq!(p.months[1].period, "2026M08");
        assert_eq!(
            p.months[1].values.get("B"),
            Some(&Cell::Number("1.1000000000000001".into()))
        );
        assert_eq!(p.months[1].values.get("C"), Some(&Cell::Text("…".into())));
    }

    #[test]
    fn finds_the_current_edition_link() {
        let page = r#"<a href="https://thedocs.worldbank.org/en/doc/74e8-0050012026/related/CMO-Historical-Data-Monthly.xlsx">x</a>"#;
        assert_eq!(
            workbook_url_in(page).as_deref(),
            Some(
                "https://thedocs.worldbank.org/en/doc/74e8-0050012026/related/CMO-Historical-Data-Monthly.xlsx"
            )
        );
        assert_eq!(workbook_url_in("<html></html>"), None);
    }
}
