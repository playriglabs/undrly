//! Normalizer for [`undrly_provider::alpaca`] IEX snapshots.
//!
//! Two quotes per symbol, each its own feed (price type) with its own time:
//!
//! - **last**: IEX's last trade, at its trade time. A trade that did not
//!   execute on IEX (exchange code other than `V`) is rejected: the feed is
//!   declared as an IEX venue quote and must not silently become anything
//!   else.
//! - **mid**: IEX's top of book, `(bid + ask) / 2` exactly (see
//!   [`undrly_core::quote::mid_price`]), with that bid and ask, at the quote
//!   time. It is IEX's own book, not a consolidated (NBBO) quote. Only a
//!   two-sided IEX book **quoted during the regular US session** (09:30 to
//!   16:00 New York time, Monday to Friday; see [`in_regular_session`])
//!   yields one: outside it IEX's book is a few resting orders, not a
//!   market. A side with no interest (price `0`, blank exchange code) or a
//!   crossed book gives no mid quote either, never a guessed one. A side
//!   from another exchange is rejected.

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use undrly_core::quote::mid_price;
use undrly_core::{BidAsk, Decimal, PriceType, VenueSymbol};
use undrly_provider::alpaca::{IEX_EXCHANGE_CODE, Quote, Snapshots};

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid, timestamp};

pub struct AlpacaNormalizer;

impl QuoteNormalizer for AlpacaNormalizer {
    type Quote = Snapshots;

    fn normalize_quotes(
        &self,
        snapshots: &Snapshots,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(snapshot) = snapshots.get(symbol.as_str()) else {
                continue;
            };
            if let Some(trade) = &snapshot.latest_trade {
                if trade.x != IEX_EXCHANGE_CODE {
                    return Err(NormalizeError::Unsupported {
                        field: "latestTrade.x",
                        value: trade.x.clone(),
                    });
                }
                out.push(NormalizedQuote {
                    symbol: symbol.clone(),
                    price_type: PriceType::Last,
                    price: decimal("latestTrade.p", &trade.p.0)?,
                    bid_ask: None,
                    observed_at: Some(timestamp("latestTrade.t", &trade.t)?),
                });
            }
            if let Some(quote) = &snapshot.latest_quote
                && let Some(ba) = two_sided(quote)?
            {
                let quoted_at = timestamp("latestQuote.t", &quote.t)?;
                if in_regular_session(quoted_at.as_datetime()) {
                    out.push(NormalizedQuote {
                        symbol: symbol.clone(),
                        price_type: PriceType::Mid,
                        price: mid_price(ba.bid, ba.ask)
                            .ok_or_else(|| invalid("latestQuote", "overflow"))?,
                        bid_ask: Some(ba),
                        observed_at: Some(quoted_at),
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Whether `at` falls in the regular US equity session: 09:30 (inclusive) to
/// 16:00 (exclusive) New York time, Monday to Friday.
///
/// New York time is UTC-4 during US daylight saving time (from the second
/// Sunday of March, 02:00 local, to the first Sunday of November, 02:00
/// local; the rule since 2007) and UTC-5 otherwise. No holiday calendar is
/// needed: IEX stamps no quotes on days it is closed. Early-close days
/// (13:00) are treated as full days.
pub fn in_regular_session(at: DateTime<Utc>) -> bool {
    let local = at + Duration::hours(if us_daylight_saving(at) { -4 } else { -5 });
    let minutes = local.hour() * 60 + local.minute();
    !matches!(local.weekday(), Weekday::Sat | Weekday::Sun)
        && (9 * 60 + 30..16 * 60).contains(&minutes)
}

/// Whether US daylight saving time is in effect at `at` (the 2007 rule).
pub fn us_daylight_saving(at: DateTime<Utc>) -> bool {
    // The `n`th Sunday of a month, at a UTC hour.
    let sunday = |month: u32, n: u32, utc_hour: u32| {
        let first = NaiveDate::from_ymd_opt(at.year(), month, 1).expect("valid month");
        let offset = (7 - first.weekday().num_days_from_sunday()) % 7;
        let day = first + Duration::days(i64::from(offset + 7 * (n - 1)));
        Utc.from_utc_datetime(&day.and_hms_opt(utc_hour, 0, 0).expect("valid hour"))
    };
    // 02:00 EST = 07:00 UTC; 02:00 EDT = 06:00 UTC.
    sunday(3, 2, 7) <= at && at < sunday(11, 1, 6)
}

/// The IEX bid and ask when both sides have IEX interest and do not cross.
fn two_sided(quote: &Quote) -> Result<Option<BidAsk>, NormalizeError> {
    let side =
        |code: &str, field: &'static str, price: &str| -> Result<Option<Decimal>, NormalizeError> {
            let price = decimal(field, price)?;
            if code.trim().is_empty() || price <= Decimal::ZERO {
                return Ok(None); // no IEX interest on this side
            }
            if code != IEX_EXCHANGE_CODE {
                return Err(NormalizeError::Unsupported {
                    field: if field == "latestQuote.bp" {
                        "latestQuote.bx"
                    } else {
                        "latestQuote.ax"
                    },
                    value: code.to_owned(),
                });
            }
            Ok(Some(price))
        };
    let bid = side(&quote.bx, "latestQuote.bp", &quote.bp.0)?;
    let ask = side(&quote.ax, "latestQuote.ap", &quote.ap.0)?;
    Ok(match (bid, ask) {
        (Some(bid), Some(ask)) if bid <= ask => Some(BidAsk { bid, ask }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;
    use undrly_provider::alpaca::AlpacaProvider;

    use super::*;

    #[test]
    fn normalizes_iex_last_trade_with_truncated_time() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/alpaca/snapshots-NVDA.json"),
        )
        .unwrap();
        let s = AlpacaProvider::new().decode_quote(&payload).unwrap();
        let q = AlpacaNormalizer
            .normalize_quotes(&s, &[VenueSymbol::new("NVDA").unwrap()])
            .unwrap();
        assert_eq!(q[0].price.to_string(), "223.71");
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-24T20:45:15.183009Z"
        );
        // The recorded book is one-sided after the close (ask 0, blank
        // exchange): no mid quote.
        assert_eq!(q.len(), 1);
    }

    fn quote_of(latest_quote: &str) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let payload = format!(
            r#"{{"NVDA":{{"latestTrade":{{"t":"2026-09-25T20:59:34.720445259Z","x":"V","p":225.05}},"latestQuote":{latest_quote}}}}}"#
        );
        let s = AlpacaProvider::new()
            .decode_quote(payload.as_bytes())
            .unwrap();
        AlpacaNormalizer.normalize_quotes(&s, &[VenueSymbol::new("NVDA").unwrap()])
    }

    #[test]
    fn a_two_sided_iex_book_in_session_is_a_mid_quote_with_its_own_time() {
        // 2026-09-25 19:59:49Z is 15:59:49 New York (EDT).
        let q = quote_of(
            r#"{"ap":225.07,"as":100,"ax":"V","bp":225.04,"bs":100,"bx":"V","c":["R"],"t":"2026-09-25T19:59:49.048604722Z","z":"R"}"#,
        )
        .unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].price_type, PriceType::Last);
        assert_eq!(q[1].price_type, PriceType::Mid);
        assert_eq!(q[1].price.to_string(), "225.055");
        let ba = q[1].bid_ask.unwrap();
        assert_eq!(
            (ba.bid.to_string(), ba.ask.to_string()),
            ("225.04".into(), "225.07".into())
        );
        assert_eq!(
            q[1].observed_at.unwrap().to_string(),
            "2026-09-25T19:59:49.048604Z"
        );
    }

    #[test]
    fn no_mid_from_a_book_quoted_outside_the_regular_session() {
        // Recorded after the 2026-09-25 close (16:59:49 EDT): IEX 225.05 /
        // 230.53, a 241 bps book of resting orders. Only the last trade.
        let q = quote_of(
            r#"{"ap":230.53,"as":100,"ax":"V","bp":225.05,"bs":100,"bx":"V","c":["R"],"t":"2026-09-25T20:59:49.048604722Z","z":"C"}"#,
        )
        .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price_type, PriceType::Last);
    }

    #[test]
    fn regular_session_is_0930_to_1600_new_york_on_weekdays() {
        let at =
            |t: &str| in_regular_session(chrono::DateTime::parse_from_rfc3339(t).unwrap().to_utc());
        // Summer (EDT, UTC-4), Friday 2026-09-25.
        assert!(!at("2026-09-25T13:29:59.999999Z"));
        assert!(at("2026-09-25T13:30:00Z"));
        assert!(at("2026-09-25T19:59:59.999999Z"));
        assert!(!at("2026-09-25T20:00:00Z"));
        // Weekend.
        assert!(!at("2026-09-26T15:00:00Z"));
        assert!(!at("2026-09-27T15:00:00Z"));
        // Winter (EST, UTC-5), Thursday 2026-01-15.
        assert!(!at("2026-01-15T13:30:00Z"));
        assert!(at("2026-01-15T14:30:00Z"));
        assert!(at("2026-01-15T20:59:59Z"));
        assert!(!at("2026-01-15T21:00:00Z"));
        // DST starts Sunday 2026-03-08: Friday 03-06 is EST, Monday 03-09 EDT.
        assert!(!at("2026-03-06T13:30:00Z"));
        assert!(at("2026-03-06T14:30:00Z"));
        assert!(at("2026-03-09T13:30:00Z"));
        // DST ends Sunday 2026-11-01: Friday 10-30 is EDT, Monday 11-02 EST.
        assert!(at("2026-10-30T13:30:00Z"));
        assert!(!at("2026-11-02T13:30:00Z"));
        assert!(at("2026-11-02T14:30:00Z"));
        // A UTC date that is still the previous New York day: Friday 23:00 EDT.
        assert!(!at("2026-09-26T03:00:00Z"));
    }

    #[test]
    fn no_mid_without_a_two_sided_uncrossed_iex_book() {
        for book in [
            r#"{"ap":0,"ax":" ","bp":210.95,"bx":"V","t":"2026-09-24T15:00:04Z"}"#,
            r#"{"ap":230.53,"ax":"V","bp":0,"bx":" ","t":"2026-09-24T15:00:04Z"}"#,
            r#"{"ap":0,"ax":"","bp":0,"bx":"","t":"2026-09-24T15:00:04Z"}"#,
            r#"{"ap":225.00,"ax":"V","bp":225.01,"bx":"V","t":"2026-09-24T15:00:04Z"}"#,
        ] {
            let q = quote_of(book).unwrap();
            assert_eq!(q.len(), 1, "{book}");
            assert_eq!(q[0].price_type, PriceType::Last);
        }
    }

    #[test]
    fn rejects_quote_sides_from_other_exchanges() {
        assert!(matches!(
            quote_of(r#"{"ap":230.53,"ax":"Q","bp":225.05,"bx":"V","t":"2026-09-24T15:00:04Z"}"#),
            Err(NormalizeError::Unsupported {
                field: "latestQuote.ax",
                ..
            })
        ));
    }

    #[test]
    fn rejects_trades_from_other_exchanges() {
        // Synthetic: a non-IEX exchange code (`Q`, Nasdaq).
        let payload =
            br#"{"NVDA":{"latestTrade":{"t":"2026-09-24T19:59:59Z","x":"Q","p":224.58}}}"#;
        let s = AlpacaProvider::new().decode_quote(payload).unwrap();
        assert!(matches!(
            AlpacaNormalizer.normalize_quotes(&s, &[VenueSymbol::new("NVDA").unwrap()]),
            Err(NormalizeError::Unsupported { .. })
        ));
    }
}
