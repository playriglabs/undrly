//! State Street (SSGA) SPY daily holdings workbook: the S&P 500 universe
//! **proxy**. These are ETF holdings, not the official S&P constituent file.
//!
//! The workbook has a short preamble (fund name, ticker, `As of <date>`), a
//! header row (`Name, Ticker, Identifier, SEDOL, Weight, Sector, Shares
//! Held, Local Currency`), one row per holding, and a legal footer. Rows are
//! returned verbatim; deciding what is a security is the builder's job.
//!
//! Terms: the file states it may not be reproduced or disclosed without
//! SSGA's written consent. Local/private use only (docs/v1.1-universe.md §12).

use undrly_core::SourceId;

use crate::DecodeError;
use crate::xlsx::{Row, read_sheet};

pub const SOURCE_ID: &str = "ssga";
/// The direct file URL (the `/us/en/intermediary/...` path redirects here).
pub const SPY_HOLDINGS_URL: &str =
    "https://www.ssga.com/library-content/products/fund-data/etfs/us/holdings-daily-us-en-spy.xlsx";
/// S&P MidCap 400 and S&P SmallCap 600 proxies (V1.10): the SPDR MDY and
/// SPSM ETFs' holdings, in the same workbook layout.
pub const MDY_HOLDINGS_URL: &str =
    "https://www.ssga.com/library-content/products/fund-data/etfs/us/holdings-daily-us-en-mdy.xlsx";
pub const SPSM_HOLDINGS_URL: &str = "https://www.ssga.com/library-content/products/fund-data/etfs/us/holdings-daily-us-en-spsm.xlsx";
pub const SHEET: &str = "holdings";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holdings {
    /// The fund ticker stated in the preamble (`SPY`).
    pub fund_ticker: Option<String>,
    /// The preamble's holdings date text, e.g. `As of 23-Sep-2026`.
    pub as_of: String,
    /// Rows after the header that have a ticker or identifier.
    pub rows: Vec<Holding>,
    /// Rows after the header with neither (legal footer, notes).
    pub other_rows: usize,
}

/// One holdings row, verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    pub row: u32,
    pub name: Option<String>,
    pub ticker: Option<String>,
    /// CUSIP for US securities.
    pub identifier: Option<String>,
    pub sedol: Option<String>,
    pub local_currency: Option<String>,
}

const HEADER: [(&str, &str); 4] = [
    ("A", "Name"),
    ("B", "Ticker"),
    ("C", "Identifier"),
    ("H", "Local Currency"),
];

pub fn decode_holdings(payload: &[u8]) -> Result<Holdings, DecodeError> {
    let reject = |reason: String| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason,
    };
    let rows = read_sheet(payload, SHEET).map_err(reject)?;
    let label = |r: &Row, label: &str| r.text("A") == Some(label);
    let fund_ticker = rows
        .iter()
        .find(|r| label(r, "Ticker Symbol:"))
        .and_then(|r| r.text("B"))
        .map(str::to_owned);
    let as_of = rows
        .iter()
        .find(|r| label(r, "Holdings:"))
        .and_then(|r| r.text("B"))
        .ok_or_else(|| reject("no `Holdings: As of <date>` line".into()))?
        .to_owned();
    let header = rows
        .iter()
        .position(|r| HEADER.iter().all(|(c, h)| r.text(c) == Some(h)))
        .ok_or_else(|| reject("no holdings header row".into()))?;
    let mut out = Vec::new();
    let mut other_rows = 0;
    for r in &rows[header + 1..] {
        let cell = |c: &str| r.text(c).map(str::to_owned);
        if cell("B").is_none() && cell("C").is_none() {
            other_rows += 1;
            continue;
        }
        out.push(Holding {
            row: r.number,
            name: cell("A"),
            ticker: cell("B"),
            identifier: cell("C"),
            sedol: cell("D"),
            local_currency: cell("H"),
        });
    }
    Ok(Holdings {
        fund_ticker,
        as_of,
        rows: out,
        other_rows,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::xlsx::tests::workbook;

    /// A synthetic holdings workbook in SSGA's layout (not SSGA data).
    pub(crate) fn synthetic() -> Vec<u8> {
        let s = |r: &str, i: usize| format!(r#"<c r="{r}" t="s"><v>{i}</v></c>"#);
        let rows = [
            format!(r#"<row r="1">{}{}</row>"#, s("A1", 0), s("B1", 1)),
            format!(r#"<row r="2">{}{}</row>"#, s("A2", 2), s("B2", 3)),
            format!(r#"<row r="3">{}{}</row>"#, s("A3", 4), s("B3", 5)),
            format!(
                r#"<row r="5">{}{}{}{}{}</row>"#,
                s("A5", 6),
                s("B5", 7),
                s("C5", 8),
                s("D5", 9),
                s("H5", 10)
            ),
            format!(
                r#"<row r="6">{}{}{}<c r="E6"><v>8.2</v></c>{}</row>"#,
                s("A6", 11),
                s("B6", 12),
                s("C6", 13),
                s("H6", 14)
            ),
            format!(
                r#"<row r="7">{}{}{}</row>"#,
                s("A7", 15),
                s("C7", 16),
                s("H7", 14)
            ),
            format!(r#"<row r="9">{}</row>"#, s("A9", 17)),
        ];
        workbook(
            &[(SHEET, &rows.concat())],
            &[
                "Fund Name:",
                "Synthetic Fund",
                "Ticker Symbol:",
                "SPY",
                "Holdings:",
                "As of 23-Sep-2026",
                "Name",
                "Ticker",
                "Identifier",
                "SEDOL",
                "Local Currency",
                "EXAMPLE CORP",
                "EXMP",
                "67066G104",
                "USD",
                "US DOLLAR",
                "CASH_USD",
                "Legal footer text.",
            ],
        )
    }

    #[test]
    fn decodes_rows_verbatim_and_counts_footer() {
        let h = decode_holdings(&synthetic()).unwrap();
        assert_eq!(h.fund_ticker.as_deref(), Some("SPY"));
        assert_eq!(h.as_of, "As of 23-Sep-2026");
        assert_eq!(h.rows.len(), 2);
        assert_eq!(h.rows[0].ticker.as_deref(), Some("EXMP"));
        assert_eq!(h.rows[0].identifier.as_deref(), Some("67066G104"));
        assert_eq!(h.rows[1].ticker, None);
        assert_eq!(h.other_rows, 1);
    }
}
