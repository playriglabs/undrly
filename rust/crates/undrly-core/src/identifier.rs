//! External identifiers and their mapping to canonical ids.
//!
//! Each namespace has its own type with its own validation and normalization;
//! there is deliberately no generic "checksummed identifier".
//!
//! | Namespace | Type | Rules |
//! | --- | --- | --- |
//! | ISIN (ISO 6166) | [`Isin`] | 2-letter prefix, 9 alphanumerics, Luhn check digit over letter-expanded digits |
//! | FIGI | [`Figi`] | 12 chars, no vowels, 3rd char `G`, reserved prefixes excluded, modified-Luhn check digit |
//! | LEI (ISO 17442) | [`Lei`] | 18 alphanumerics + 2 check digits, ISO 7064 MOD 97-10 |
//! | MIC (ISO 10383) | [`Mic`] | 4 uppercase alphanumerics; no check digit |
//! | ISO 4217 | [`CurrencyCode`] | 3 uppercase letters; no check digit |
//! | SEC CIK | [`Cik`] | 10 digits, zero-padded, not all zeros; no check digit |
//!
//! `parse` accepts only the canonical spelling. `normalize` applies the
//! namespace's normalization first: trim and uppercase for the alphanumeric
//! namespaces (case-insensitive by specification), trim and zero-pad for CIK.
//! Venue symbols are *not* normalized: see [`crate::VenueSymbol`].
//!
//! Registry membership (e.g. whether a MIC or currency code is currently
//! assigned) is reference data, not syntax, and is not checked here.

use std::fmt;

use crate::id::{CanonicalId, Category, ListingId, VenueId};
use crate::reference::VenueSymbol;
use crate::source::Provenance;
use crate::time::Validity;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {namespace} `{value}`: {reason}")]
pub struct IdentifierError {
    pub namespace: Namespace,
    pub value: String,
    pub reason: &'static str,
}

/// External identifier namespaces with global (unscoped) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Namespace {
    Isin,
    Figi,
    Lei,
    Mic,
    Iso4217,
    Cik,
}

impl Namespace {
    pub const ALL: [Namespace; 6] = [
        Namespace::Isin,
        Namespace::Figi,
        Namespace::Lei,
        Namespace::Mic,
        Namespace::Iso4217,
        Namespace::Cik,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Namespace::Isin => "isin",
            Namespace::Figi => "figi",
            Namespace::Lei => "lei",
            Namespace::Mic => "mic",
            Namespace::Iso4217 => "iso4217",
            Namespace::Cik => "cik",
        }
    }

    /// Categories a value in this namespace may identify. FIGIs exist at
    /// share-class/composite level (instrument) and exchange level (listing).
    /// A CIK identifies an SEC filer, which Undrly models as an entity.
    pub const fn categories(self) -> &'static [Category] {
        match self {
            Namespace::Isin => &[Category::Instrument],
            Namespace::Figi => &[Category::Instrument, Category::Listing],
            Namespace::Lei => &[Category::Entity],
            Namespace::Mic => &[Category::Venue],
            Namespace::Iso4217 => &[Category::Currency],
            Namespace::Cik => &[Category::Entity],
        }
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn error(namespace: Namespace, value: &str, reason: &'static str) -> IdentifierError {
    IdentifierError {
        namespace,
        value: value.to_owned(),
        reason,
    }
}

/// Value of an alphanumeric character in check-digit schemes: 0–9, A=10 … Z=35.
fn char_value(b: u8) -> u32 {
    match b {
        b'0'..=b'9' => u32::from(b - b'0'),
        _ => u32::from(b - b'A') + 10,
    }
}

fn sum_digits(mut n: u32) -> u32 {
    let mut sum = 0;
    while n > 0 {
        sum += n % 10;
        n /= 10;
    }
    sum
}

macro_rules! identifier_type {
    ($(#[$meta:meta])* $name:ident, $namespace:expr, $validate:path) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Box<str>);

        impl $name {
            pub const NAMESPACE: Namespace = $namespace;

            /// Accepts only the canonical spelling.
            pub fn parse(s: &str) -> Result<Self, IdentifierError> {
                $validate(s.as_bytes())
                    .map(|()| Self(s.into()))
                    .map_err(|reason| error(Self::NAMESPACE, s, reason))
            }

            /// Trims surrounding whitespace and uppercases, then parses.
            pub fn normalize(s: &str) -> Result<Self, IdentifierError> {
                Self::parse(&s.trim().to_ascii_uppercase())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

fn validate_isin(b: &[u8]) -> Result<(), &'static str> {
    if b.len() != 12 {
        return Err("must be 12 characters");
    }
    if !b[..2].iter().all(u8::is_ascii_uppercase) {
        return Err("must start with a 2-letter prefix");
    }
    if !b[2..11]
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err("characters 3-11 must be uppercase alphanumeric");
    }
    if !b[11].is_ascii_digit() {
        return Err("check digit must be numeric");
    }
    // Expand letters to two digits, then Luhn over the expanded digit string.
    let mut digits = Vec::with_capacity(22);
    for &c in &b[..11] {
        let v = char_value(c);
        if v >= 10 {
            digits.push(v / 10);
        }
        digits.push(v % 10);
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| if i % 2 == 0 { sum_digits(d * 2) } else { d })
        .sum();
    if (10 - sum % 10) % 10 == u32::from(b[11] - b'0') {
        Ok(())
    } else {
        Err("check digit mismatch")
    }
}

const FIGI_RESERVED_PREFIXES: [&[u8; 2]; 7] = [b"BS", b"BM", b"GG", b"GB", b"GH", b"KY", b"VG"];

fn validate_figi(b: &[u8]) -> Result<(), &'static str> {
    if b.len() != 12 {
        return Err("must be 12 characters");
    }
    let allowed = |c: &u8| c.is_ascii_digit() || (c.is_ascii_uppercase() && !b"AEIOU".contains(c));
    if !b[..11].iter().all(allowed) {
        return Err("characters 1-11 must be digits or uppercase consonants");
    }
    if FIGI_RESERVED_PREFIXES.iter().any(|p| &b[..2] == *p) {
        return Err("reserved prefix");
    }
    if b[2] != b'G' {
        return Err("third character must be `G`");
    }
    if !b[11].is_ascii_digit() {
        return Err("check digit must be numeric");
    }
    let sum: u32 = b[..11]
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let v = char_value(c);
            sum_digits(if i % 2 == 1 { v * 2 } else { v })
        })
        .sum();
    if (10 - sum % 10) % 10 == u32::from(b[11] - b'0') {
        Ok(())
    } else {
        Err("check digit mismatch")
    }
}

fn validate_lei(b: &[u8]) -> Result<(), &'static str> {
    if b.len() != 20 {
        return Err("must be 20 characters");
    }
    if !b[..18]
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err("characters 1-18 must be uppercase alphanumeric");
    }
    if !b[18..].iter().all(u8::is_ascii_digit) {
        return Err("check digits must be numeric");
    }
    // ISO 7064 MOD 97-10: the letter-expanded number mod 97 must equal 1.
    let remainder = b.iter().fold(0u32, |acc, &c| {
        let v = char_value(c);
        if v >= 10 {
            (acc * 100 + v) % 97
        } else {
            (acc * 10 + v) % 97
        }
    });
    if remainder == 1 {
        Ok(())
    } else {
        Err("check digits mismatch")
    }
}

fn validate_mic(b: &[u8]) -> Result<(), &'static str> {
    if b.len() == 4
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        Ok(())
    } else {
        Err("must be 4 uppercase alphanumeric characters")
    }
}

fn validate_currency_code(b: &[u8]) -> Result<(), &'static str> {
    if b.len() == 3 && b.iter().all(u8::is_ascii_uppercase) {
        Ok(())
    } else {
        Err("must be 3 uppercase letters")
    }
}

identifier_type!(
    /// International Securities Identification Number (ISO 6166).
    Isin,
    Namespace::Isin,
    validate_isin
);
identifier_type!(
    /// Financial Instrument Global Identifier.
    Figi,
    Namespace::Figi,
    validate_figi
);
identifier_type!(
    /// Legal Entity Identifier (ISO 17442).
    Lei,
    Namespace::Lei,
    validate_lei
);
identifier_type!(
    /// Market Identifier Code (ISO 10383).
    Mic,
    Namespace::Mic,
    validate_mic
);
identifier_type!(
    /// ISO 4217 alphabetic currency code. A display/lookup code, not identity.
    CurrencyCode,
    Namespace::Iso4217,
    validate_currency_code
);

/// SEC Central Index Key: the identifier EDGAR assigns to a filer.
///
/// Canonical spelling is EDGAR's 10-digit zero-padded form
/// (`0001045810`), as used in EDGAR URLs and submissions data. A CIK has no
/// check digit, so validation is shape only; whether a CIK is assigned is
/// registry data. CIK 0 is never assigned and is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cik(Box<str>);

impl Cik {
    pub const NAMESPACE: Namespace = Namespace::Cik;
    const WIDTH: usize = 10;

    /// Accepts only the canonical spelling: exactly 10 ASCII digits.
    pub fn parse(s: &str) -> Result<Self, IdentifierError> {
        let reject = |reason| error(Self::NAMESPACE, s, reason);
        if s.len() != Self::WIDTH || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(reject("must be 10 digits"));
        }
        if s.bytes().all(|b| b == b'0') {
            return Err(reject("must not be zero"));
        }
        Ok(Self(s.into()))
    }

    /// Trims surrounding whitespace and left-pads 1–10 digits with zeros
    /// (EDGAR often writes CIKs unpadded, e.g. `1045810`), then parses. No
    /// other spelling (prefixes, signs, separators) is accepted.
    pub fn normalize(s: &str) -> Result<Self, IdentifierError> {
        let trimmed = s.trim();
        if trimmed.is_empty()
            || trimmed.len() > Self::WIDTH
            || !trimmed.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(error(Self::NAMESPACE, s, "must be 1 to 10 digits"));
        }
        Self::parse(&format!("{trimmed:0>10}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Cik {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated external identifier from a global namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExternalIdentifier {
    Isin(Isin),
    Figi(Figi),
    Lei(Lei),
    Mic(Mic),
    Iso4217(CurrencyCode),
    Cik(Cik),
}

impl ExternalIdentifier {
    pub fn namespace(&self) -> Namespace {
        match self {
            ExternalIdentifier::Isin(_) => Namespace::Isin,
            ExternalIdentifier::Figi(_) => Namespace::Figi,
            ExternalIdentifier::Lei(_) => Namespace::Lei,
            ExternalIdentifier::Mic(_) => Namespace::Mic,
            ExternalIdentifier::Iso4217(_) => Namespace::Iso4217,
            ExternalIdentifier::Cik(_) => Namespace::Cik,
        }
    }

    pub fn value(&self) -> &str {
        match self {
            ExternalIdentifier::Isin(v) => v.as_str(),
            ExternalIdentifier::Figi(v) => v.as_str(),
            ExternalIdentifier::Lei(v) => v.as_str(),
            ExternalIdentifier::Mic(v) => v.as_str(),
            ExternalIdentifier::Iso4217(v) => v.as_str(),
            ExternalIdentifier::Cik(v) => v.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{namespace} cannot identify a {category} node")]
pub struct AssignmentError {
    pub namespace: Namespace,
    pub category: Category,
}

/// A source's claim that an external identifier refers to a canonical node
/// during a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierAssignment {
    identifier: ExternalIdentifier,
    node: CanonicalId,
    valid_during: Validity,
    provenance: Provenance,
}

impl IdentifierAssignment {
    pub fn new(
        identifier: ExternalIdentifier,
        node: CanonicalId,
        valid_during: Validity,
        provenance: Provenance,
    ) -> Result<Self, AssignmentError> {
        let namespace = identifier.namespace();
        if !namespace.categories().contains(&node.category()) {
            return Err(AssignmentError {
                namespace,
                category: node.category(),
            });
        }
        Ok(Self {
            identifier,
            node,
            valid_during,
            provenance,
        })
    }

    pub fn identifier(&self) -> &ExternalIdentifier {
        &self.identifier
    }

    pub fn node(&self) -> CanonicalId {
        self.node
    }

    pub fn valid_during(&self) -> Validity {
        self.valid_during
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// True when both claim the same identifier for overlapping periods but
    /// for different nodes. Conflicts are quarantined, never auto-resolved.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.identifier == other.identifier
            && self.node != other.node
            && self.valid_during.overlaps(&other.valid_during)
    }
}

/// A venue-scoped symbol assigned to a listing during a period. The venue must
/// be the listing's venue (enforced by storage, which knows the listing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingSymbol {
    pub listing_id: ListingId,
    pub venue_id: VenueId,
    pub symbol: VenueSymbol,
    pub valid_during: Validity,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{CurrencyId, InstrumentId};
    use crate::source::SourceId;
    use crate::time::Timestamp;

    #[test]
    fn isin_check_digit() {
        for valid in [
            "US0378331005",
            "US67066G1040",
            "GB0002634946",
            "DE000BAY0017",
        ] {
            assert!(Isin::parse(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "US0378331006", // wrong check digit
            "US67066G1041",
            "us0378331005", // lowercase is not canonical
            "US037833100",
            "1S0378331005",
            "US037833100A",
        ] {
            assert!(Isin::parse(invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            Isin::normalize(" us0378331005 ").unwrap().as_str(),
            "US0378331005"
        );
    }

    #[test]
    fn figi_rules() {
        for valid in ["BBG000BLNNH6", "BBG000B9XRY4", "BBG000BBJQV0"] {
            assert!(Figi::parse(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "BBG000BLNNH7", // wrong check digit
            "BBG000BLNNHA", // non-numeric check digit
            "BBA000BLNNH6", // third character not G
            "BBG000BLANH6", // vowel
            "BSG000BLNNH6", // reserved prefix
            "BBG000BLNNH",
        ] {
            assert!(Figi::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn lei_mod_97() {
        for valid in ["HWUPKR0MPOU8FGXBT394", "5493001KJTIIGC8Y1R12"] {
            assert!(Lei::parse(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "HWUPKR0MPOU8FGXBT395",
            "HWUPKR0MPOU8FGXBT39",
            "hwupkr0mpou8fgxbt394",
        ] {
            assert!(Lei::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn mic_and_currency_are_shape_only() {
        assert!(Mic::parse("XNAS").is_ok());
        assert!(Mic::parse("xnas").is_err());
        assert_eq!(Mic::normalize("xnas").unwrap().as_str(), "XNAS");
        assert!(Mic::parse("XNA").is_err());
        assert!(CurrencyCode::parse("USD").is_ok());
        assert!(CurrencyCode::parse("US1").is_err());
        assert!(CurrencyCode::parse("USDT").is_err());
    }

    #[test]
    fn cik_rules() {
        assert_eq!(Cik::parse("0001045810").unwrap().as_str(), "0001045810");
        for invalid in [
            "1045810",     // not padded: not canonical
            "00001045810", // 11 digits
            "0000000000",  // zero is never assigned
            "000104581A",
            " 0001045810",
            "",
        ] {
            assert!(Cik::parse(invalid).is_err(), "{invalid:?}");
        }
        for (raw, canonical) in [
            ("1045810", "0001045810"),
            (" 0001045810 ", "0001045810"),
            ("320193", "0000320193"),
            ("9999999999", "9999999999"),
        ] {
            assert_eq!(Cik::normalize(raw).unwrap().as_str(), canonical, "{raw:?}");
        }
        for invalid in [
            "0",
            "CIK0001045810",
            "-1045810",
            "+1045810",
            "1,045,810",
            "10458100000",
            "",
        ] {
            assert!(Cik::normalize(invalid).is_err(), "{invalid:?}");
        }
        let cik = ExternalIdentifier::Cik(Cik::parse("0001045810").unwrap());
        assert_eq!(
            (cik.namespace().as_str(), cik.value()),
            ("cik", "0001045810")
        );
        assert_eq!(Namespace::Cik.categories(), &[Category::Entity]);
    }

    fn provenance() -> Provenance {
        Provenance {
            source_id: SourceId::parse("example-source").unwrap(),
            received_at: Timestamp::parse("2026-09-24T12:00:00Z").unwrap(),
        }
    }

    #[test]
    fn assignment_checks_category() {
        let usd = ExternalIdentifier::Iso4217(CurrencyCode::parse("USD").unwrap());
        assert!(
            IdentifierAssignment::new(
                usd.clone(),
                CurrencyId::generate().into(),
                Validity::UNBOUNDED,
                provenance()
            )
            .is_ok()
        );
        assert_eq!(
            IdentifierAssignment::new(
                usd,
                InstrumentId::generate().into(),
                Validity::UNBOUNDED,
                provenance()
            ),
            Err(AssignmentError {
                namespace: Namespace::Iso4217,
                category: Category::Instrument,
            })
        );
    }

    #[test]
    fn detects_conflicts() {
        let isin = ExternalIdentifier::Isin(Isin::parse("US0378331005").unwrap());
        let a = InstrumentId::generate().into();
        let b = InstrumentId::generate().into();
        let t = |s: &str| Some(Timestamp::parse(s).unwrap());
        let early = Validity::new(None, t("2020-01-01T00:00:00Z")).unwrap();
        let late = Validity::new(t("2020-01-01T00:00:00Z"), None).unwrap();
        let assign = |node, validity| {
            IdentifierAssignment::new(isin.clone(), node, validity, provenance()).unwrap()
        };
        assert!(assign(a, Validity::UNBOUNDED).conflicts_with(&assign(b, late)));
        assert!(!assign(a, early).conflicts_with(&assign(b, late)));
        assert!(!assign(a, Validity::UNBOUNDED).conflicts_with(&assign(a, late)));
    }
}
