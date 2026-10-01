//! Normalizers for the V1.9 stablecoin/fiat venues (docs/v1.9-live-fx.md):
//! Binance, Coins.ph, OKX, Indodax, Bitkub and HashKey.
//!
//! Each is a venue's own order book: one **mid** observation with the
//! venue's best bid and ask, at the venue's own time when the payload states
//! one (OKX, Indodax, HashKey), else none (receipt time is used downstream).
//! A payload that names its markets is matched to the requested symbols;
//! one that names none (Indodax) is attributed to the single requested
//! symbol, and more than one is an error.

use undrly_core::quote::mid_price;
use undrly_core::{BidAsk, Decimal, PriceType, Timestamp, VenueSymbol};
use undrly_provider::{binance, bitkub, hashkey, indodax, okx};

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid};

fn positive(field: &'static str, text: &str) -> Result<Decimal, NormalizeError> {
    let value = decimal(field, text)?;
    if value > Decimal::ZERO {
        Ok(value)
    } else {
        Err(invalid(field, format!("`{text}` is not a positive price")))
    }
}

fn unix_millis(field: &'static str, ms: i64) -> Result<Timestamp, NormalizeError> {
    let micros = ms
        .checked_mul(1_000)
        .ok_or_else(|| invalid(field, "out of range"))?;
    Timestamp::from_unix_micros(micros).map_err(|e| invalid(field, e))
}

/// One venue book quote: the mid of the best bid and ask, with both.
fn book(
    symbol: &VenueSymbol,
    bid: &str,
    ask: &str,
    at: Option<Timestamp>,
) -> Result<NormalizedQuote, NormalizeError> {
    let bid = positive("bid", bid)?;
    let ask = positive("ask", ask)?;
    Ok(NormalizedQuote {
        symbol: symbol.clone(),
        price_type: PriceType::Mid,
        price: mid_price(bid, ask).ok_or_else(|| invalid("bid/ask", "overflow"))?,
        bid_ask: Some(BidAsk { bid, ask }),
        observed_at: at,
    })
}

/// The requested symbols that a named payload has, in request order.
fn named<'a, T>(
    symbols: &'a [VenueSymbol],
    rows: &'a [T],
    name: impl Fn(&T) -> &str,
) -> impl Iterator<Item = (&'a VenueSymbol, &'a T)> {
    symbols
        .iter()
        .filter_map(move |s| rows.iter().find(|r| name(r) == s.as_str()).map(|r| (s, r)))
}

// ------------------------------------------------- Binance-style book tickers

/// Binance and Coins.ph `bookTicker` → one mid per requested symbol. The
/// payload states no time.
pub struct BookTickerNormalizer;

impl QuoteNormalizer for BookTickerNormalizer {
    type Quote = binance::BookTickers;

    fn normalize_quotes(
        &self,
        t: &binance::BookTickers,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        named(symbols, &t.0, |r| r.symbol.as_str())
            .map(|(s, r)| book(s, &r.bid_price, &r.ask_price, None))
            .collect()
    }
}

// ---------------------------------------------------------------- OKX

/// OKX ticker → one mid per requested instrument, at the ticker's `ts`.
pub struct OkxNormalizer;

impl QuoteNormalizer for OkxNormalizer {
    type Quote = okx::Tickers;

    fn normalize_quotes(
        &self,
        t: &okx::Tickers,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        named(symbols, &t.0, |r| r.inst_id.as_str())
            .map(|(s, r)| {
                let ms: i64 = r.ts.trim().parse().map_err(|e| invalid("ts", e))?;
                book(s, &r.bid_px, &r.ask_px, Some(unix_millis("ts", ms)?))
            })
            .collect()
    }
}

// ---------------------------------------------------------------- Indodax

/// Indodax ticker (`buy` = best bid, `sell` = best ask) → one mid for the
/// single requested pair, at `server_time`.
pub struct IndodaxNormalizer;

impl QuoteNormalizer for IndodaxNormalizer {
    type Quote = indodax::Ticker;

    fn normalize_quotes(
        &self,
        t: &indodax::Ticker,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let symbol = match symbols {
            [] => return Ok(Vec::new()),
            [one] => one,
            _ => {
                return Err(invalid(
                    "pair",
                    "a ticker names no pair; request one pair per record",
                ));
            }
        };
        let at = unix_millis(
            "server_time",
            t.server_time
                .checked_mul(1_000)
                .ok_or_else(|| invalid("server_time", "out of range"))?,
        )?;
        Ok(vec![book(symbol, &t.buy, &t.sell, Some(at))?])
    }
}

// ---------------------------------------------------------------- Bitkub

/// Bitkub ticker → one mid per requested symbol. The payload states no time.
pub struct BitkubNormalizer;

impl QuoteNormalizer for BitkubNormalizer {
    type Quote = bitkub::Tickers;

    fn normalize_quotes(
        &self,
        t: &bitkub::Tickers,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        named(symbols, &t.0, |r| r.symbol.as_str())
            .map(|(s, r)| book(s, &r.highest_bid, &r.lowest_ask, None))
            .collect()
    }
}

// ---------------------------------------------------------------- HashKey

/// HashKey `bookTicker` → one mid per requested symbol, at `t`.
pub struct HashKeyNormalizer;

impl QuoteNormalizer for HashKeyNormalizer {
    type Quote = hashkey::BookTickers;

    fn normalize_quotes(
        &self,
        t: &hashkey::BookTickers,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        named(symbols, &t.0, |r| r.symbol.as_str())
            .map(|(s, r)| book(s, &r.bid, &r.ask, Some(unix_millis("t", r.time)?)))
            .collect()
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

    fn sym(s: &str) -> VenueSymbol {
        VenueSymbol::new(s).unwrap()
    }

    fn assert_book(q: &NormalizedQuote) {
        let ba = q.bid_ask.expect("bid and ask");
        assert_eq!(q.price_type, PriceType::Mid);
        assert!(ba.bid <= q.price && q.price <= ba.ask, "{q:?}");
    }

    #[test]
    fn binance_book_tickers_for_requested_symbols_only() {
        let t = binance::BinanceProvider::new()
            .decode_quote(&fixture("binance/book-ticker.json"))
            .unwrap();
        let q = BookTickerNormalizer
            .normalize_quotes(&t, &[sym("USDCIDR"), sym("USDTIDR"), sym("NOPE")])
            .unwrap();
        assert_eq!(
            q.iter().map(|q| q.symbol.as_str()).collect::<Vec<_>>(),
            ["USDCIDR", "USDTIDR"]
        );
        assert!(q.iter().all(|q| q.observed_at.is_none()), "no time stated");
        q.iter().for_each(assert_book);
    }

    #[test]
    fn okx_hashkey_and_indodax_carry_the_venue_time() {
        let o = okx::OkxProvider::new()
            .decode_quote(&fixture("okx/ticker-USDT-SGD.json"))
            .unwrap();
        let q = OkxNormalizer
            .normalize_quotes(&o, &[sym("USDT-SGD")])
            .unwrap();
        assert!(q[0].observed_at.is_some());
        assert_book(&q[0]);

        let h = hashkey::HashKeyProvider::new()
            .decode_quote(&fixture("hashkey/book-ticker-USDTHKD.json"))
            .unwrap();
        let q = HashKeyNormalizer
            .normalize_quotes(&h, &[sym("USDTHKD")])
            .unwrap();
        assert!(q[0].observed_at.is_some());
        assert_book(&q[0]);

        let i = indodax::IndodaxProvider::new()
            .decode_quote(&fixture("indodax/ticker-usdtidr.json"))
            .unwrap();
        let q = IndodaxNormalizer
            .normalize_quotes(&i, &[sym("usdtidr")])
            .unwrap();
        assert_eq!(
            q[0].observed_at.unwrap().as_datetime().timestamp(),
            i.server_time
        );
        assert_book(&q[0]);
        assert!(
            IndodaxNormalizer
                .normalize_quotes(&i, &[sym("usdtidr"), sym("usdcidr")])
                .is_err(),
            "an unnamed payload is one pair"
        );
    }

    #[test]
    fn bitkub_and_coins_ph_books() {
        let b = bitkub::BitkubProvider::new()
            .decode_quote(&fixture("bitkub/ticker-USDT_THB.json"))
            .unwrap();
        let q = BitkubNormalizer
            .normalize_quotes(&b, &[sym("USDT_THB")])
            .unwrap();
        assert_book(&q[0]);

        let c = undrly_provider::coins_ph::CoinsPhProvider::new()
            .decode_quote(&fixture("coins-ph/book-ticker-USDTPHP.json"))
            .unwrap();
        let q = BookTickerNormalizer
            .normalize_quotes(&c, &[sym("USDTPHP")])
            .unwrap();
        assert_book(&q[0]);
    }

    #[test]
    fn zero_or_garbage_prices_are_rejected() {
        let zero = binance::BookTickers(vec![binance::BookTicker {
            symbol: "USDTIDR".into(),
            bid_price: "0".into(),
            ask_price: "1".into(),
        }]);
        assert!(
            BookTickerNormalizer
                .normalize_quotes(&zero, &[sym("USDTIDR")])
                .is_err()
        );
    }
}
