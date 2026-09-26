//! Normalizer for [`undrly_provider::worldbank`] monthly prices.
//!
//! Price type **`average`**: a monthly average, neither spot nor futures.
//! Each feed symbol is one of the World Bank's Pink Sheet series codes,
//! mapped here to the workbook's exact series name and unit; a column whose
//! name or unit differs is an error, never a guess.
//!
//! - Value: the newest month with a number. The workbook stores numbers as
//!   IEEE doubles written with 17 significant digits (`1.1000000000000001`);
//!   they are rounded **in decimal** to 15 significant digits (the precision
//!   a double reliably carries, and what Excel displays) and trailing zeros
//!   are dropped, giving `1.1`.
//! - Time: the end of the averaging period, i.e. 00:00 UTC on the first day
//!   of the following month.

use undrly_core::{Decimal, PriceType, Timestamp, VenueSymbol};
use undrly_provider::worldbank::MonthlyPrices;
use undrly_provider::xlsx::Cell;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, invalid};

pub struct WorldBankNormalizer;

/// Series code → (series name in the workbook, unit).
pub const SERIES: [(&str, &str, &str); 7] = [
    ("MAIZE", "Maize", "($/mt)"),
    ("WHEAT_US_HRW", "Wheat, US HRW", "($/mt)"),
    ("SOYBEANS", "Soybeans", "($/mt)"),
    ("COFFEE_ARABIC", "Coffee, Arabica", "($/kg)"),
    ("SUGAR_WLD", "Sugar, world", "($/kg)"),
    ("COCOA", "Cocoa", "($/kg)"),
    ("COTTON_A_INDX", "Cotton, A Index", "($/kg)"),
];

/// A stored double's text, rounded to 15 significant digits in decimal.
pub fn stored_double(text: &str) -> Result<Decimal, NormalizeError> {
    let t = text.trim();
    let parsed = if t.contains(['e', 'E']) {
        Decimal::from_scientific(t)
    } else {
        t.parse::<Decimal>()
    }
    .map_err(|e| invalid("value", format!("`{t}`: {e}")))?;
    let rounded = parsed
        .round_sf(15)
        .ok_or_else(|| invalid("value", format!("`{t}` cannot be rounded")))?;
    Ok(rounded.normalize())
}

/// `2026M08` → 2026-09-01T00:00:00Z (the end of August).
fn period_end(period: &str) -> Result<Timestamp, NormalizeError> {
    let bad = || invalid("period", format!("`{period}`"));
    let (year, month) = period.split_once('M').ok_or_else(bad)?;
    let (year, month): (i32, u32) = (
        year.parse().map_err(|_| bad())?,
        month.parse().map_err(|_| bad())?,
    );
    if !(1..=12).contains(&month) {
        return Err(bad());
    }
    let (y, m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Timestamp::parse(&format!("{y:04}-{m:02}-01T00:00:00Z")).map_err(|e| invalid("period", e))
}

impl QuoteNormalizer for WorldBankNormalizer {
    type Quote = MonthlyPrices;

    /// The newest month with a value, per series.
    fn normalize_quotes(
        &self,
        p: &MonthlyPrices,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, p, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for WorldBankNormalizer {
    /// Every month with a value, per series.
    fn normalize_history(
        &self,
        p: &MonthlyPrices,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some((_, name, unit)) = SERIES.iter().find(|(c, _, _)| *c == symbol.as_str())
            else {
                return Err(NormalizeError::Unsupported {
                    field: "series",
                    value: symbol.as_str().to_owned(),
                });
            };
            let Some((column, stated)) = p.column(name) else {
                continue;
            };
            if stated != *unit {
                return Err(invalid(
                    "unit",
                    format!("{name} is in {stated}, expected {unit}"),
                ));
            }
            for m in &p.months {
                if let Some(Cell::Number(v)) = m.values.get(column) {
                    out.push(NormalizedQuote {
                        symbol: symbol.clone(),
                        price_type: PriceType::Average,
                        price: stored_double(v)?,
                        bid_ask: None,
                        observed_at: Some(period_end(&m.period)?),
                    });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use undrly_provider::worldbank::Month;

    use super::*;

    #[test]
    fn rounds_stored_doubles_in_decimal() {
        for (stored, expected) in [
            ("1.1000000000000001", "1.1"),
            ("72.311000000000007", "72.311"),
            ("0.94089999999999996", "0.9409"),
            ("6.6600000000000006E-2", "0.0666"),
            ("224", "224"),
            ("135.19999999999999", "135.2"),
        ] {
            assert_eq!(stored_double(stored).unwrap().to_string(), expected);
        }
        assert!(stored_double("…").is_err());
    }

    fn prices(unit: &str) -> MonthlyPrices {
        let month = |period: &str, v: Cell| Month {
            period: period.into(),
            values: BTreeMap::from([("AE".to_owned(), v)]),
        };
        MonthlyPrices {
            updated: None,
            series: BTreeMap::from([("AE".to_owned(), ("Maize".to_owned(), unit.to_owned()))]),
            months: vec![
                month("2026M07", Cell::Number("221.30000000000001".into())),
                month("2026M08", Cell::Text("…".into())),
            ],
        }
    }

    #[test]
    fn newest_number_as_monthly_average_at_period_end() {
        let maize = VenueSymbol::new("MAIZE").unwrap();
        let q = WorldBankNormalizer
            .normalize_quotes(&prices("($/mt)"), std::slice::from_ref(&maize))
            .unwrap();
        assert_eq!(q[0].price.to_string(), "221.3");
        assert_eq!(q[0].price_type, PriceType::Average);
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-08-01T00:00:00Z"
        );
        assert!(
            WorldBankNormalizer
                .normalize_quotes(&prices("($/kg)"), std::slice::from_ref(&maize))
                .is_err()
        );
        assert_eq!(
            period_end("2026M12").unwrap().to_string(),
            "2027-01-01T00:00:00Z"
        );
        assert!(
            WorldBankNormalizer
                .normalize_quotes(&prices("($/mt)"), &[VenueSymbol::new("RICE").unwrap()])
                .is_err()
        );
    }
}
