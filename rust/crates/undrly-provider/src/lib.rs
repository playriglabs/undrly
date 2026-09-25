//! Provider boundary.
//!
//! ```text
//! provider payload → decode (this crate) → validate → normalize → canonical domain
//! ```
//!
//! A provider turns its own payloads into *provider-native* records: its own
//! symbols, its own field meanings, exact decimals. It never constructs
//! canonical objects; mapping provider records to canonical identities is
//! normalization, which is a separate step owned elsewhere.
//!
//! Capabilities are separate traits so a provider implements only what it
//! actually supports: [`QuoteProvider`] and [`ReferenceDataProvider`] today.
//! `InstrumentProvider`, `SearchProvider`, and `StreamingProvider` are added
//! when the first provider that genuinely supports them is integrated.
//!
//! Implementations:
//! - [`fixture`]: deterministic reference-data provider over Undrly's own
//!   fixture format; no network.
//! - [`sec`]: SEC EDGAR company submissions (filer identity), the first real
//!   source.
//!
//! Decoding is synchronous and operates on bytes, so every provider is
//! testable against captured fixtures with no network. Transport lives
//! behind the `http` feature ([`sec::http`]), and this crate is the only one
//! that performs network access: `undrly-core`, `undrly-normalize`, and
//! `undrly-store` never do.

use undrly_core::SourceId;

pub mod fixture;
pub mod sec;

/// A payload could not be decoded into a provider-native record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{source_id}: {reason}")]
pub struct DecodeError {
    pub source_id: SourceId,
    pub reason: String,
}

/// Every provider identifies the source whose data it decodes.
pub trait Provider {
    fn source_id(&self) -> &SourceId;
}

/// Capability: decode quote/price payloads.
pub trait QuoteProvider: Provider {
    /// Provider-native quote record. Must use exact decimals for prices.
    type Quote;

    fn decode_quote(&self, payload: &[u8]) -> Result<Self::Quote, DecodeError>;
}

/// Capability: decode reference-data payloads (issuers, securities,
/// listings, identifiers) into provider-native records.
pub trait ReferenceDataProvider: Provider {
    /// Provider-native reference record: the source's own field meanings and
    /// unvalidated values. Validation into canonical types is normalization.
    type Record;

    fn decode_reference(&self, payload: &[u8]) -> Result<Self::Record, DecodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use undrly_core::VenueSymbol;

    /// Test double for an imaginary source whose payload is `SYMBOL,PRICE`.
    struct StubProvider {
        source_id: SourceId,
    }

    #[derive(Debug, PartialEq)]
    struct StubQuote {
        symbol: VenueSymbol,
        price: Decimal,
    }

    impl Provider for StubProvider {
        fn source_id(&self) -> &SourceId {
            &self.source_id
        }
    }

    impl QuoteProvider for StubProvider {
        type Quote = StubQuote;

        fn decode_quote(&self, payload: &[u8]) -> Result<StubQuote, DecodeError> {
            let reject = |reason: &str| DecodeError {
                source_id: self.source_id.clone(),
                reason: reason.to_owned(),
            };
            let text = std::str::from_utf8(payload).map_err(|_| reject("payload is not UTF-8"))?;
            let (symbol, price) = text
                .split_once(',')
                .ok_or_else(|| reject("expected SYMBOL,PRICE"))?;
            Ok(StubQuote {
                symbol: VenueSymbol::new(symbol).map_err(|e| reject(&e.to_string()))?,
                price: undrly_core::decimal::parse_canonical(price)
                    .map_err(|e| reject(&e.to_string()))?,
            })
        }
    }

    fn provider() -> StubProvider {
        StubProvider {
            source_id: SourceId::parse("stub-source").unwrap(),
        }
    }

    #[test]
    fn decodes_into_provider_native_record() {
        let quote = provider().decode_quote(b"NVDA,183.4200").unwrap();
        assert_eq!(quote.symbol.as_str(), "NVDA");
        assert_eq!(quote.price.to_string(), "183.4200");
    }

    #[test]
    fn rejects_malformed_payloads() {
        for payload in [&b"NVDA"[..], b"NVDA,1e3", b"NV DA,1.0", b"\xff,1.0"] {
            let err = provider().decode_quote(payload).unwrap_err();
            assert_eq!(err.source_id.as_str(), "stub-source");
        }
    }
}
