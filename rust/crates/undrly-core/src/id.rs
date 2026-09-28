//! Canonical Undrly identifiers.
//!
//! ```text
//! undrly:<category>:<id>
//! ```
//!
//! - `category` is the structural node type ([`Category`]). It never encodes
//!   asset class, jurisdiction, venue, or symbol.
//! - `id` is a generated UUIDv7 written as 26 characters of lowercase Crockford
//!   base32 (the TypeID encoding; the first character is `0`–`7`).
//!
//! Canonical identity is never derived from names or external identifiers
//! (tickers, ISINs, FIGIs, ...); those map to canonical ids through the
//! identifier layer ([`crate::identifier`]). Parsing is strict, so every id has
//! exactly one spelling.

use std::fmt;
use std::str::FromStr;

use uuid::{Uuid, Variant};

const SCHEME: &str = "undrly";
const ENCODED_LEN: usize = 26;
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Structural node type of a canonical id. Immutable for the life of the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Entity,
    Instrument,
    Listing,
    Venue,
    /// Fiat currency. Crypto assets are instruments.
    Currency,
    /// A blockchain network, identified by its CAIP-2 chain id.
    Chain,
    /// An asset's existence on one chain (a token contract, a mint, or the
    /// chain's native asset). Never the economic asset itself.
    Deployment,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Entity,
        Category::Instrument,
        Category::Listing,
        Category::Venue,
        Category::Currency,
        Category::Chain,
        Category::Deployment,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Entity => "entity",
            Category::Instrument => "instrument",
            Category::Listing => "listing",
            Category::Venue => "venue",
            Category::Currency => "currency",
            Category::Chain => "chain",
            Category::Deployment => "deployment",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Category {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| IdError::UnknownCategory(s.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("identifier must have the form `undrly:<category>:<id>`")]
    Malformed,
    #[error("unknown identifier category `{0}`")]
    UnknownCategory(String),
    #[error("identifier body is not canonical lowercase base32 of 26 characters")]
    InvalidEncoding,
    #[error("identifier is not an RFC 9562 UUIDv7")]
    NotUuidV7,
    #[error("expected {expected} identifier, found {found}")]
    WrongCategory { expected: Category, found: Category },
}

fn check_v7(uuid: Uuid) -> Result<Uuid, IdError> {
    if uuid.get_version_num() == 7 && uuid.get_variant() == Variant::RFC4122 {
        Ok(uuid)
    } else {
        Err(IdError::NotUuidV7)
    }
}

fn encode(uuid: Uuid) -> String {
    let value = uuid.as_u128();
    (0..ENCODED_LEN)
        .map(|i| {
            let shift = 5 * (ENCODED_LEN - 1 - i);
            char::from(ALPHABET[((value >> shift) & 0x1f) as usize])
        })
        .collect()
}

fn decode(text: &str) -> Result<Uuid, IdError> {
    let bytes = text.as_bytes();
    // 26 × 5 = 130 bits; the top two must be zero, so the first digit is <= 7.
    if bytes.len() != ENCODED_LEN || !(b'0'..=b'7').contains(&bytes[0]) {
        return Err(IdError::InvalidEncoding);
    }
    let mut value: u128 = 0;
    for &b in bytes {
        let digit = ALPHABET
            .iter()
            .position(|&a| a == b)
            .ok_or(IdError::InvalidEncoding)?;
        value = (value << 5) | digit as u128;
    }
    Ok(Uuid::from_u128(value))
}

/// A validated canonical identifier of any category.
///
/// Use the typed ids ([`EntityId`], [`InstrumentId`], ...) where the category
/// is known; `CanonicalId` is for places where any node may appear, such as
/// relationship endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanonicalId {
    category: Category,
    uuid: Uuid,
}

impl CanonicalId {
    /// Wraps an existing UUIDv7 (e.g. read from storage).
    pub fn from_parts(category: Category, uuid: Uuid) -> Result<Self, IdError> {
        Ok(Self {
            category,
            uuid: check_v7(uuid)?,
        })
    }

    pub fn parse(s: &str) -> Result<Self, IdError> {
        let mut parts = s.split(':');
        let (Some(SCHEME), Some(category), Some(body), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(IdError::Malformed);
        };
        Self::from_parts(category.parse()?, decode(body)?)
    }

    pub fn category(&self) -> Category {
        self.category
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCHEME}:{}:{}", self.category, encode(self.uuid))
    }
}

impl FromStr for CanonicalId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Orders by canonical text, which for a fixed category is UUIDv7 order.
impl Ord for CanonicalId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.category.as_str(), self.uuid).cmp(&(other.category.as_str(), other.uuid))
    }
}

impl PartialOrd for CanonicalId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident, $category:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            pub const CATEGORY: Category = $category;

            /// Mints a new identifier. This is the only way to create
            /// canonical identity; it is never derived from other data.
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUIDv7 (e.g. read from storage).
            pub fn from_uuid(uuid: Uuid) -> Result<Self, IdError> {
                check_v7(uuid).map(Self)
            }

            pub fn parse(s: &str) -> Result<Self, IdError> {
                CanonicalId::parse(s)?.try_into()
            }

            pub fn uuid(&self) -> Uuid {
                self.0
            }

            pub fn canonical(&self) -> CanonicalId {
                CanonicalId {
                    category: Self::CATEGORY,
                    uuid: self.0,
                }
            }
        }

        impl TryFrom<CanonicalId> for $name {
            type Error = IdError;

            fn try_from(id: CanonicalId) -> Result<Self, Self::Error> {
                if id.category == Self::CATEGORY {
                    Ok(Self(id.uuid))
                } else {
                    Err(IdError::WrongCategory {
                        expected: Self::CATEGORY,
                        found: id.category,
                    })
                }
            }
        }

        impl From<$name> for CanonicalId {
            fn from(id: $name) -> Self {
                id.canonical()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.canonical().fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }
    };
}

typed_id!(
    /// Economic or legal entity, e.g. a company.
    EntityId,
    Category::Entity
);
typed_id!(
    /// Financial instrument, including crypto assets.
    InstrumentId,
    Category::Instrument
);
typed_id!(
    /// An instrument's listing on a venue.
    ListingId,
    Category::Listing
);
typed_id!(
    /// Trading venue.
    VenueId,
    Category::Venue
);
typed_id!(
    /// Fiat currency.
    CurrencyId,
    Category::Currency
);
typed_id!(
    /// Blockchain network.
    ChainId,
    Category::Chain
);
typed_id!(
    /// An asset's deployment on one chain.
    DeploymentId,
    Category::Deployment
);

#[cfg(test)]
mod tests {
    use super::*;

    const V7: &str = "0192a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b";

    fn v7() -> Uuid {
        Uuid::parse_str(V7).unwrap()
    }

    #[test]
    fn round_trips_text() {
        let id = CanonicalId::from_parts(Category::Instrument, v7()).unwrap();
        let text = id.to_string();
        assert!(text.starts_with("undrly:instrument:"), "{text}");
        assert_eq!(text.len(), "undrly:instrument:".len() + 26);
        assert_eq!(CanonicalId::parse(&text).unwrap(), id);
    }

    #[test]
    fn encoding_is_order_preserving() {
        let mut ids: Vec<InstrumentId> = (0..50).map(|_| InstrumentId::generate()).collect();
        let mut texts: Vec<String> = ids.iter().map(ToString::to_string).collect();
        ids.sort();
        texts.sort();
        let sorted: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(sorted, texts);
    }

    #[test]
    fn generated_ids_are_v7_and_unique() {
        let a = EntityId::generate();
        let b = EntityId::generate();
        assert_ne!(a, b);
        assert_eq!(a.uuid().get_version_num(), 7);
        assert_eq!(EntityId::parse(&a.to_string()).unwrap(), a);
    }

    #[test]
    fn rejects_non_v7_uuids() {
        let v4 = Uuid::parse_str("4f1c2d3e-0000-4000-8000-000000000009").unwrap();
        assert_eq!(InstrumentId::from_uuid(v4), Err(IdError::NotUuidV7));
        assert_eq!(
            InstrumentId::from_uuid(Uuid::nil()),
            Err(IdError::NotUuidV7)
        );
        let wrong_variant = Uuid::parse_str("0192a1b2-c3d4-7e5f-ca6b-7c8d9e0f1a2b").unwrap();
        assert_eq!(
            InstrumentId::from_uuid(wrong_variant),
            Err(IdError::NotUuidV7)
        );
    }

    #[test]
    fn rejects_malformed_text() {
        let body = CanonicalId::from_parts(Category::Entity, v7())
            .unwrap()
            .to_string()
            .rsplit(':')
            .next()
            .unwrap()
            .to_owned();
        let cases = [
            String::new(),
            "NVDA".into(),
            "undrly:equity:US:NVDA".into(),
            format!("UNDRLY:entity:{body}"),
            format!("undrly:Entity:{body}"),
            format!("undrly:company:{body}"),
            format!("undrly:entity:{}", body.to_uppercase()),
            format!("undrly:entity:{}", &body[1..]),
            format!("undrly:entity:{body}0"),
            format!("undrly:entity:8{}", &body[1..]),
            format!("undrly:entity:{}u", &body[..25]),
            format!("undrly:entity:{body}:x"),
            format!(" undrly:entity:{body}"),
        ];
        for case in cases {
            assert!(CanonicalId::parse(&case).is_err(), "{case:?}");
        }
    }

    #[test]
    fn typed_ids_enforce_category() {
        let venue = VenueId::generate();
        assert_eq!(
            InstrumentId::parse(&venue.to_string()),
            Err(IdError::WrongCategory {
                expected: Category::Instrument,
                found: Category::Venue,
            })
        );
        assert_eq!(VenueId::parse(&venue.to_string()), Ok(venue));
        assert_eq!(CanonicalId::from(venue).category(), Category::Venue);
    }

    #[test]
    fn categories_round_trip() {
        for c in Category::ALL {
            assert_eq!(c.as_str().parse::<Category>(), Ok(c));
        }
        assert!("token".parse::<Category>().is_err());
    }
}
