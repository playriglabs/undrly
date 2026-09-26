//! Normalizers for the V1.2 FX sources (docs/v1.2-fx.md).
//!
//! - [`BitstampNormalizer`]: a venue's own book, **mid** with bid and ask.
//! - Every other source publishes **reference** rates (price type
//!   `reference`, no bid/ask): the ECB, the Bank of Canada, the Federal
//!   Reserve (H.10), Bank Indonesia, Bank Negara Malaysia and the Central Bank
//!   of Myanmar. Buy/sell or buying/selling columns some of them publish are
//!   the institution's own rates, not a market spread, and are never passed
//!   on as a bid/ask.
//!
//! Each rate is returned in the orientation the source publishes, per one
//! unit of the priced currency (a rate "per 100 units" is divided by 100,
//! exactly). Turning a source's orientation into the canonical pair's
//! (inversion) is the feed's declared job at ingestion, never done here.
//!
//! Sources that state only a date (ECB, Bank of Canada, H.10) get
//! `observed_at` = that date at 00:00 UTC, as for EIA: the date is the
//! source's, the time of day is a convention, and Undrly's clock is never
//! used.

use chrono::{NaiveDate, TimeZone, Utc};
use undrly_core::quote::mid_price;
use undrly_core::{BidAsk, Decimal, PriceType, Timestamp, VenueSymbol};
use undrly_provider::{bank_indonesia, bank_of_canada, bitstamp, bnm, cbm, ecb, fed_h10};

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid, timestamp};

/// `YYYY-MM-DD` → that date at 00:00 UTC.
fn date_start(field: &'static str, date: &str) -> Result<Timestamp, NormalizeError> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| invalid(field, e))?;
    let t = Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight exists"));
    Timestamp::from_datetime(t).map_err(|e| invalid(field, e))
}

/// Unix seconds as text → a timestamp.
fn unix_seconds(field: &'static str, text: &str) -> Result<Timestamp, NormalizeError> {
    let secs: i64 = text.trim().parse().map_err(|e| invalid(field, e))?;
    let micros = secs
        .checked_mul(1_000_000)
        .ok_or_else(|| invalid(field, "out of range"))?;
    Timestamp::from_unix_micros(micros).map_err(|e| invalid(field, e))
}

/// A rate stated for `units` units, per one unit (exact decimal division).
fn per_unit(field: &'static str, rate: Decimal, units: Decimal) -> Result<Decimal, NormalizeError> {
    if units <= Decimal::ZERO {
        return Err(invalid(field, "units must be positive"));
    }
    if units == Decimal::ONE {
        return Ok(rate);
    }
    rate.checked_div(units)
        .map(|v| v.normalize())
        .ok_or_else(|| invalid(field, "cannot divide by units"))
}

fn positive(field: &'static str, value: Decimal) -> Result<Decimal, NormalizeError> {
    if value > Decimal::ZERO {
        Ok(value)
    } else {
        Err(invalid(field, format!("`{value}` is not a positive rate")))
    }
}

fn reference(symbol: &VenueSymbol, price: Decimal, at: Timestamp) -> NormalizedQuote {
    NormalizedQuote {
        symbol: symbol.clone(),
        price_type: PriceType::Reference,
        price,
        bid_ask: None,
        observed_at: Some(at),
    }
}

// ---------------------------------------------------------------- Bitstamp

/// Bitstamp ticker → one **mid** observation with the venue's best bid and
/// ask and the ticker's own time. The payload names no market, so it is
/// attributed to the single requested symbol (more than one is an error).
pub struct BitstampNormalizer;

impl QuoteNormalizer for BitstampNormalizer {
    type Quote = bitstamp::Ticker;

    fn normalize_quotes(
        &self,
        t: &bitstamp::Ticker,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let symbol = match symbols {
            [] => return Ok(Vec::new()),
            [one] => one,
            _ => {
                return Err(invalid(
                    "market",
                    "a ticker names no market; request one market per record",
                ));
            }
        };
        let bid = positive("bid", decimal("bid", &t.bid)?)?;
        let ask = positive("ask", decimal("ask", &t.ask)?)?;
        Ok(vec![NormalizedQuote {
            symbol: symbol.clone(),
            price_type: PriceType::Mid,
            price: mid_price(bid, ask).ok_or_else(|| invalid("bid/ask", "overflow"))?,
            bid_ask: Some(BidAsk { bid, ask }),
            observed_at: Some(unix_seconds("timestamp", &t.timestamp)?),
        }])
    }
}

// ---------------------------------------------------------------- ECB

/// ECB reference rates: symbol = ISO code (`JPY` is JPY per 1 EUR, i.e.
/// EUR/JPY).
pub struct EcbNormalizer;

impl QuoteNormalizer for EcbNormalizer {
    type Quote = ecb::ReferenceRates;

    /// The newest day's rate of each requested code.
    fn normalize_quotes(
        &self,
        r: &ecb::ReferenceRates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, r, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for EcbNormalizer {
    /// Every day's rate of each requested code.
    fn normalize_history(
        &self,
        r: &ecb::ReferenceRates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for (date, rates) in &r.days {
            let at = date_start("time", date)?;
            for symbol in symbols {
                if let Some((_, rate)) = rates.iter().find(|(c, _)| c == symbol.as_str()) {
                    let price = positive("rate", decimal("rate", rate)?)?;
                    out.push(reference(symbol, price, at));
                }
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- Bank of Canada

/// Bank of Canada Valet: symbol = series (`FXJPYCAD` is CAD per 1 JPY), its
/// newest dated value.
pub struct BankOfCanadaNormalizer;

impl QuoteNormalizer for BankOfCanadaNormalizer {
    type Quote = bank_of_canada::Observations;

    fn normalize_quotes(
        &self,
        o: &bank_of_canada::Observations,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, o, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for BankOfCanadaNormalizer {
    /// Every dated value of each requested series.
    fn normalize_history(
        &self,
        o: &bank_of_canada::Observations,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        use bank_of_canada::ObservationValue;
        let mut out = Vec::new();
        for symbol in symbols {
            for row in &o.observations {
                if let (Some(ObservationValue::Date(d)), Some(ObservationValue::Value { v })) =
                    (row.get("d"), row.get(symbol.as_str()))
                {
                    let price = positive("v", decimal("v", v)?)?;
                    out.push(reference(symbol, price, date_start("d", d)?));
                }
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- Fed H.10

/// Federal Reserve H.10: symbol = series (`RXI_N.B.JA` is JPY per 1 USD,
/// `RXI$US_N.B.NZ` is USD per 1 NZD), its newest business day with a value
/// (`ND` rows are skipped). Every series must have multiplier 1.
pub struct FedH10Normalizer;

impl QuoteNormalizer for FedH10Normalizer {
    type Quote = fed_h10::H10;

    fn normalize_quotes(
        &self,
        h: &fed_h10::H10,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, h, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for FedH10Normalizer {
    /// Every business day with a value (`ND` skipped) of each series.
    fn normalize_history(
        &self,
        h: &fed_h10::H10,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(column) = h.series.iter().position(|s| s == symbol.as_str()) else {
                continue;
            };
            if h.multipliers[column] != "1" {
                return Err(invalid(
                    "Multiplier",
                    format!("{symbol}: multiplier `{}`", h.multipliers[column]),
                ));
            }
            for (date, values) in &h.rows {
                if values[column] == "ND" {
                    continue;
                }
                let price = positive("value", decimal("value", &values[column])?)?;
                out.push(reference(symbol, price, date_start("Time Period", date)?));
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- Bank Indonesia

/// Bank Indonesia: `JISDOR-USD` (IDR per 1 USD, the single JISDOR rate) or
/// `KURS-XXX` (IDR per 1 XXX: the middle of Bank Indonesia's transaction
/// rates). The newest dated row of the currency; a symbol whose series is
/// not the response's is not in it.
pub struct BankIndonesiaNormalizer;

impl QuoteNormalizer for BankIndonesiaNormalizer {
    type Quote = bank_indonesia::Rates;

    fn normalize_quotes(
        &self,
        r: &bank_indonesia::Rates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, r, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for BankIndonesiaNormalizer {
    /// Every dated row of each requested currency.
    fn normalize_history(
        &self,
        r: &bank_indonesia::Rates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        use bank_indonesia::Series;
        let mut out = Vec::new();
        for symbol in symbols {
            let Some((series, currency)) = symbol.as_str().split_once('-') else {
                return Err(invalid("symbol", format!("`{symbol}` is not SERIES-CCY")));
            };
            let wanted = match series {
                "JISDOR" => Series::Jisdor,
                "KURS" => Series::Transaction,
                _ => return Err(invalid("symbol", format!("unknown series in `{symbol}`"))),
            };
            if wanted != r.series {
                continue;
            }
            for row in r.rows.iter().filter(|x| x.currency == currency) {
                let buy = positive("beli", decimal("beli", &row.buy)?)?;
                let sell = positive("jual", decimal("jual", &row.sell)?)?;
                let rate = match r.series {
                    Series::Jisdor if buy == sell => buy,
                    Series::Jisdor => {
                        return Err(invalid("JISDOR", "buy and sell differ; expected one rate"));
                    }
                    // Kurs tengah: the middle of Bank Indonesia's buy/sell rates.
                    Series::Transaction => {
                        mid_price(buy, sell).ok_or_else(|| invalid("beli/jual", "overflow"))?
                    }
                };
                let units = decimal("nil", &row.units)?;
                out.push(reference(
                    symbol,
                    per_unit("nil", rate, units)?,
                    timestamp("tgl", &row.date)?,
                ));
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- BNM

/// Bank Negara Malaysia: symbol = ISO code (`USD` is MYR per 1 USD), the
/// middle rate of the stated session, per one unit. Doubles are rounded in
/// decimal to 15 significant digits (as for the World Bank). Only the `rm`
/// quotation (MYR per foreign unit) is accepted.
pub struct BnmNormalizer;

impl QuoteNormalizer for BnmNormalizer {
    type Quote = bnm::Rates;

    fn normalize_quotes(
        &self,
        r: &bnm::Rates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        if r.meta.quote != "rm" {
            return Err(NormalizeError::Unsupported {
                field: "meta.quote",
                value: r.meta.quote.clone(),
            });
        }
        let s = &r.meta.session;
        if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("meta.session", format!("`{s}` is not HHMM")));
        }
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(rate) = r.data.iter().find(|x| x.currency_code == symbol.as_str()) else {
                continue;
            };
            let Some(middle) = &rate.rate.middle_rate else {
                continue;
            };
            let value = positive("middle_rate", crate::worldbank::stored_double(&middle.0)?)?;
            let at = timestamp(
                "rate.date",
                &format!("{}T{}:{}:00+08:00", rate.rate.date, &s[..2], &s[2..]),
            )?;
            out.push(reference(
                symbol,
                per_unit("unit", value, Decimal::from(rate.unit))?,
                at,
            ));
        }
        Ok(out)
    }
}

/// BNM's one-currency month history: every business day's middle rate,
/// per one unit, at the stated session. Symbol = the currency code.
pub struct BnmMonthNormalizer;

impl QuoteNormalizer for BnmMonthNormalizer {
    type Quote = bnm::MonthRates;

    fn normalize_quotes(
        &self,
        r: &bnm::MonthRates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, r, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for BnmMonthNormalizer {
    fn normalize_history(
        &self,
        r: &bnm::MonthRates,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        if r.meta.quote != "rm" {
            return Err(NormalizeError::Unsupported {
                field: "meta.quote",
                value: r.meta.quote.clone(),
            });
        }
        let s = &r.meta.session;
        if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("meta.session", format!("`{s}` is not HHMM")));
        }
        let Some(symbol) = symbols.iter().find(|x| x.as_str() == r.data.currency_code) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for day in &r.data.rate {
            let Some(middle) = &day.middle_rate else {
                continue;
            };
            let value = positive("middle_rate", crate::worldbank::stored_double(&middle.0)?)?;
            let at = timestamp(
                "rate.date",
                &format!("{}T{}:{}:00+08:00", day.date, &s[..2], &s[2..]),
            )?;
            out.push(reference(
                symbol,
                per_unit("unit", value, Decimal::from(r.data.unit))?,
                at,
            ));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- CBM

/// Central Bank of Myanmar: symbol = ISO code, MMK per one unit. The
/// payload states no units, so only currencies whose unit is known are
/// accepted (USD, per 1 USD); any other requested symbol is an error.
pub struct CbmNormalizer;

/// Currencies whose CBM rate is known to be per one unit.
const CBM_PER_ONE: [&str; 1] = ["USD"];

impl QuoteNormalizer for CbmNormalizer {
    type Quote = cbm::Latest;

    fn normalize_quotes(
        &self,
        l: &cbm::Latest,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let at = unix_seconds("timestamp", &l.timestamp)?;
        let mut out = Vec::new();
        for symbol in symbols {
            if !CBM_PER_ONE.contains(&symbol.as_str()) {
                return Err(invalid(
                    "rates",
                    format!("{symbol}: CBM states no unit for this currency"),
                ));
            }
            if let Some(rate) = l.rates.get(symbol.as_str()) {
                let price = positive("rate", decimal("rate", rate)?)?;
                out.push(reference(symbol, price, at));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;

    use super::*;

    fn fixture(path: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources")
                .join(path),
        )
        .unwrap()
    }

    fn syms(s: &[&str]) -> Vec<VenueSymbol> {
        s.iter().map(|x| VenueSymbol::new(x).unwrap()).collect()
    }

    fn one(q: &[NormalizedQuote], symbol: &str) -> (String, Option<(String, String)>, String) {
        let x = q.iter().find(|x| x.symbol.as_str() == symbol).unwrap();
        (
            x.price.to_string(),
            x.bid_ask.map(|b| (b.bid.to_string(), b.ask.to_string())),
            x.observed_at.unwrap().to_string(),
        )
    }

    #[test]
    fn bitstamp_mid_with_bid_ask_and_ticker_time() {
        let t = bitstamp::BitstampProvider::new()
            .decode_quote(&fixture("bitstamp/ticker-eurusd.json"))
            .unwrap();
        let q = BitstampNormalizer
            .normalize_quotes(&t, &syms(&["eurusd"]))
            .unwrap();
        assert_eq!(q[0].price_type, PriceType::Mid);
        assert_eq!(
            one(&q, "eurusd"),
            (
                "1.138825".into(),
                Some(("1.13882".into(), "1.13883".into())),
                "2026-09-26T03:30:00Z".into()
            )
        );
        assert!(
            BitstampNormalizer
                .normalize_quotes(&t, &syms(&["eurusd", "gbpusd"]))
                .is_err()
        );
        let zero = bitstamp::Ticker {
            bid: "0".into(),
            ..t
        };
        assert!(
            BitstampNormalizer
                .normalize_quotes(&zero, &syms(&["eurusd"]))
                .is_err()
        );
    }

    #[test]
    fn history_returns_every_dated_value() {
        use crate::HistoryNormalizer;
        let h = fed_h10::FedH10Provider::new()
            .decode_quote(&fixture("fed-h10/h10-lastobs10.csv"))
            .unwrap();
        let all = FedH10Normalizer
            .normalize_history(&h, &syms(&["RXI_N.B.JA"]))
            .unwrap();
        // 10 rows, one `ND` (a US holiday): 9 values, the newest the quote.
        assert_eq!(all.len(), 9);
        let newest = FedH10Normalizer
            .normalize_quotes(&h, &syms(&["RXI_N.B.JA"]))
            .unwrap();
        assert_eq!(newest.len(), 1);
        assert_eq!(
            newest[0].observed_at,
            all.iter().map(|q| q.observed_at).max().unwrap()
        );
        let o = bank_of_canada::BankOfCanadaProvider::new()
            .decode_quote(&fixture("bank-of-canada/observations.json"))
            .unwrap();
        assert_eq!(
            BankOfCanadaNormalizer
                .normalize_history(&o, &syms(&["FXJPYCAD"]))
                .unwrap()
                .len(),
            5
        );
        let j = bank_indonesia::BankIndonesiaProvider::new()
            .decode_quote(&fixture("bank-indonesia/jisdor-usd.xml"))
            .unwrap();
        assert_eq!(
            BankIndonesiaNormalizer
                .normalize_history(&j, &syms(&["JISDOR-USD"]))
                .unwrap()
                .len(),
            10
        );
        let m = bnm::BnmMonthProvider::new()
            .decode_quote(
                br#"{"data":{"currency_code":"USD","unit":1,"rate":[
                {"date":"2026-09-01","buying_rate":4.03,"selling_rate":4.04,"middle_rate":4.0359999999999996},
                {"date":"2026-09-02","buying_rate":null,"selling_rate":null,"middle_rate":null}]},
               "meta":{"quote":"rm","session":"1700"}}"#,
            )
            .unwrap();
        let q = BnmMonthNormalizer
            .normalize_history(&m, &syms(&["USD"]))
            .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price.to_string(), "4.036");
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-01T09:00:00Z"
        );
    }

    #[test]
    fn ecb_reference_rates_at_the_stated_date() {
        let r = ecb::EcbProvider::new()
            .decode_quote(&fixture("ecb/eurofxref-daily.xml"))
            .unwrap();
        let q = EcbNormalizer
            .normalize_quotes(&r, &syms(&["JPY", "SGD", "XXX"]))
            .unwrap();
        assert_eq!(q.len(), 2, "an absent code is not returned");
        assert_eq!(q[0].price_type, PriceType::Reference);
        assert_eq!(
            one(&q, "JPY"),
            ("179.70".into(), None, "2026-09-25T00:00:00Z".into())
        );
        assert_eq!(one(&q, "SGD").0, "1.4563");
    }

    #[test]
    fn bank_of_canada_newest_value_per_series() {
        let o = bank_of_canada::BankOfCanadaProvider::new()
            .decode_quote(&fixture("bank-of-canada/observations.json"))
            .unwrap();
        let q = BankOfCanadaNormalizer
            .normalize_quotes(&o, &syms(&["FXJPYCAD", "FXEURCAD"]))
            .unwrap();
        assert_eq!(
            one(&q, "FXJPYCAD"),
            ("0.008990".into(), None, "2026-09-25T00:00:00Z".into())
        );
        assert_eq!(one(&q, "FXEURCAD").0, "1.6122");
    }

    #[test]
    fn fed_h10_newest_business_day_skipping_nd() {
        let h = fed_h10::FedH10Provider::new()
            .decode_quote(&fixture("fed-h10/h10-lastobs10.csv"))
            .unwrap();
        let q = FedH10Normalizer
            .normalize_quotes(&h, &syms(&["RXI_N.B.JA", "RXI$US_N.B.NZ", "RXI_N.B.SI"]))
            .unwrap();
        assert_eq!(
            one(&q, "RXI_N.B.JA"),
            ("156.8700".into(), None, "2026-09-18T00:00:00Z".into())
        );
        assert_eq!(one(&q, "RXI$US_N.B.NZ").0, "0.5710");
        assert_eq!(one(&q, "RXI_N.B.SI").0, "1.2775");
        let mut all_nd = h.clone();
        for (_, v) in &mut all_nd.rows {
            v.iter_mut().for_each(|x| *x = "ND".into());
        }
        assert!(
            FedH10Normalizer
                .normalize_quotes(&all_nd, &syms(&["RXI_N.B.JA"]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn bank_indonesia_jisdor_and_transaction_middle() {
        let j = bank_indonesia::BankIndonesiaProvider::new()
            .decode_quote(&fixture("bank-indonesia/jisdor-usd.xml"))
            .unwrap();
        let q = BankIndonesiaNormalizer
            .normalize_quotes(&j, &syms(&["JISDOR-USD", "KURS-SGD"]))
            .unwrap();
        assert_eq!(q.len(), 1, "KURS-SGD is not in a JISDOR response");
        assert_eq!(
            one(&q, "JISDOR-USD"),
            ("17917.00".into(), None, "2026-09-24T17:00:00Z".into())
        );
        let k = bank_indonesia::BankIndonesiaProvider::new()
            .decode_quote(&fixture("bank-indonesia/kurs-sgd.xml"))
            .unwrap();
        let q = BankIndonesiaNormalizer
            .normalize_quotes(&k, &syms(&["KURS-SGD"]))
            .unwrap();
        // (13921.60 + 14062.61) / 2, exact; no bid/ask is exposed.
        assert_eq!(
            one(&q, "KURS-SGD"),
            ("13992.105".into(), None, "2026-09-24T17:00:00Z".into())
        );
        // Per-100-unit rows are divided exactly.
        let mut jpy = k.clone();
        jpy.rows[0].currency = "JPY".into();
        jpy.rows[0].units = "100.00".into();
        let q = BankIndonesiaNormalizer
            .normalize_quotes(&jpy, &syms(&["KURS-JPY"]))
            .unwrap();
        assert_eq!(one(&q, "KURS-JPY").0, "139.92105");
    }

    #[test]
    fn bnm_middle_rate_per_unit_at_the_session_time() {
        let r = bnm::BnmProvider::new()
            .decode_quote(&fixture("bnm/exchange-rate-1700.json"))
            .unwrap();
        let q = BnmNormalizer
            .normalize_quotes(&r, &syms(&["USD", "SGD", "IDR"]))
            .unwrap();
        assert_eq!(
            one(&q, "USD"),
            ("4.0735".into(), None, "2026-09-25T09:00:00Z".into())
        );
        assert_eq!(one(&q, "SGD").0, "3.1878");
        // IDR is quoted per 100 IDR: 0.0228 / 100.
        assert_eq!(one(&q, "IDR").0, "0.000228");
        let mut fx = r.clone();
        fx.meta.quote = "fx".into();
        assert!(
            BnmNormalizer
                .normalize_quotes(&fx, &syms(&["USD"]))
                .is_err()
        );
    }

    #[test]
    fn cbm_accepts_only_currencies_with_a_known_unit() {
        let l = cbm::CbmProvider::new()
            .decode_quote(&fixture("cbm/latest.json"))
            .unwrap();
        let q = CbmNormalizer.normalize_quotes(&l, &syms(&["USD"])).unwrap();
        assert_eq!(
            one(&q, "USD"),
            ("2100.00".into(), None, "2026-09-25T08:00:00Z".into())
        );
        assert!(CbmNormalizer.normalize_quotes(&l, &syms(&["IDR"])).is_err());
    }
}
