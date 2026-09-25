//! Directed, provenance-carrying relationships between canonical nodes.
//!
//! A relationship reads `subject TYPE object`, e.g.
//! `<NVDA instrument> ISSUED_BY <NVIDIA entity>`. Only one canonical direction
//! is ever stored: `dependent → thing it depends on`. Inverses (for example
//! `UNDERLYING_OF`, the inverse of [`RelationshipType::DerivesFrom`]) are labels
//! derived at query time and are not part of this vocabulary. `LISTED_ON` is
//! projected from [`Listing`](crate::Listing)s rather than stored as an edge.

use std::fmt;
use std::str::FromStr;

use crate::id::{CanonicalId, Category};
use crate::source::Provenance;

/// Storable relationship vocabulary, in canonical direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipType {
    /// instrument → issuing entity.
    IssuedBy,
    /// instrument → venue where it trades.
    TradesOn,
    /// instrument → currency or asset it is denominated in.
    DenominatedIn,
    /// instrument → currency or asset it settles in.
    SettlesIn,
    /// derivative → underlying. Inverse view: `UNDERLYING_OF`.
    DerivesFrom,
    /// fund → instrument it holds.
    Holds,
    /// fund → index or instrument it tracks.
    Tracks,
    /// instrument → index it is a member of.
    MemberOf,
    /// token → instrument it tokenizes.
    Tokenizes,
    /// token → asset it represents.
    Represents,
    /// instrument → oracle/reference feed pricing it.
    PricedBy,
    /// instrument → venue or chain where it is available.
    AvailableOn,
    /// Fallback when no precise type applies.
    RelatedTo,
}

impl RelationshipType {
    pub const ALL: [RelationshipType; 13] = [
        RelationshipType::IssuedBy,
        RelationshipType::TradesOn,
        RelationshipType::DenominatedIn,
        RelationshipType::SettlesIn,
        RelationshipType::DerivesFrom,
        RelationshipType::Holds,
        RelationshipType::Tracks,
        RelationshipType::MemberOf,
        RelationshipType::Tokenizes,
        RelationshipType::Represents,
        RelationshipType::PricedBy,
        RelationshipType::AvailableOn,
        RelationshipType::RelatedTo,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            RelationshipType::IssuedBy => "ISSUED_BY",
            RelationshipType::TradesOn => "TRADES_ON",
            RelationshipType::DenominatedIn => "DENOMINATED_IN",
            RelationshipType::SettlesIn => "SETTLES_IN",
            RelationshipType::DerivesFrom => "DERIVES_FROM",
            RelationshipType::Holds => "HOLDS",
            RelationshipType::Tracks => "TRACKS",
            RelationshipType::MemberOf => "MEMBER_OF",
            RelationshipType::Tokenizes => "TOKENIZES",
            RelationshipType::Represents => "REPRESENTS",
            RelationshipType::PricedBy => "PRICED_BY",
            RelationshipType::AvailableOn => "AVAILABLE_ON",
            RelationshipType::RelatedTo => "RELATED_TO",
        }
    }

    /// Allowed `(subject, object)` categories. Empty means the type is not
    /// storable yet: the node categories or classes it needs (fund, index,
    /// derivative, token, chain, oracle feed) do not exist, or, for
    /// `RELATED_TO`, no symmetric-ordering rule exists. Unknown is preferable
    /// to wrong.
    ///
    /// Must equal the `relationship_rules` table; `undrly-store` tests this.
    pub const fn allowed_endpoints(self) -> &'static [(Category, Category)] {
        use Category::{Currency, Entity, Instrument, Venue};
        match self {
            RelationshipType::IssuedBy => &[(Instrument, Entity)],
            RelationshipType::TradesOn => &[(Instrument, Venue)],
            RelationshipType::DenominatedIn | RelationshipType::SettlesIn => {
                &[(Instrument, Currency), (Instrument, Instrument)]
            }
            RelationshipType::DerivesFrom
            | RelationshipType::Holds
            | RelationshipType::Tracks
            | RelationshipType::MemberOf
            | RelationshipType::Tokenizes
            | RelationshipType::Represents
            | RelationshipType::PricedBy
            | RelationshipType::AvailableOn
            | RelationshipType::RelatedTo => &[],
        }
    }
}

impl fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationshipError {
    #[error("unknown relationship type `{0}`")]
    UnknownType(String),
    #[error("relationship subject and object are the same node")]
    SelfReference,
    #[error("{relationship_type} cannot connect a {subject} to a {object}")]
    InvalidEndpoints {
        relationship_type: RelationshipType,
        subject: Category,
        object: Category,
    },
}

impl FromStr for RelationshipType {
    type Err = RelationshipError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| RelationshipError::UnknownType(s.to_owned()))
    }
}

/// A directed relationship as currently asserted by one source. Provenance is
/// mandatory; an unsourced relationship cannot be constructed. Validity
/// periods are not modeled yet: this is a current assertion, not eternal
/// historical truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    subject: CanonicalId,
    relationship_type: RelationshipType,
    object: CanonicalId,
    provenance: Provenance,
}

impl Relationship {
    pub fn new(
        subject: CanonicalId,
        relationship_type: RelationshipType,
        object: CanonicalId,
        provenance: Provenance,
    ) -> Result<Self, RelationshipError> {
        if subject == object {
            return Err(RelationshipError::SelfReference);
        }
        let endpoints = (subject.category(), object.category());
        if !relationship_type.allowed_endpoints().contains(&endpoints) {
            return Err(RelationshipError::InvalidEndpoints {
                relationship_type,
                subject: endpoints.0,
                object: endpoints.1,
            });
        }
        Ok(Self {
            subject,
            relationship_type,
            object,
            provenance,
        })
    }

    pub fn subject(&self) -> CanonicalId {
        self.subject
    }

    pub fn relationship_type(&self) -> RelationshipType {
        self.relationship_type
    }

    pub fn object(&self) -> CanonicalId {
        self.object
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{CurrencyId, EntityId, InstrumentId, VenueId};
    use crate::source::SourceId;
    use crate::time::Timestamp;

    fn provenance() -> Provenance {
        Provenance {
            source_id: SourceId::parse("example-source").unwrap(),
            received_at: Timestamp::parse("2026-09-24T12:00:00Z").unwrap(),
        }
    }

    #[test]
    fn vocabulary_round_trips_and_excludes_inverses() {
        for t in RelationshipType::ALL {
            assert_eq!(t.as_str().parse::<RelationshipType>(), Ok(t));
        }
        for not_stored in ["UNDERLYING_OF", "LISTED_ON", "issued_by"] {
            assert!(not_stored.parse::<RelationshipType>().is_err());
        }
    }

    #[test]
    fn canonical_direction_is_enforced() {
        let nvda: CanonicalId = InstrumentId::generate().into();
        let nvidia: CanonicalId = EntityId::generate().into();
        let rel =
            Relationship::new(nvda, RelationshipType::IssuedBy, nvidia, provenance()).unwrap();
        assert_eq!(rel.subject(), nvda);
        assert_eq!(rel.object(), nvidia);
        assert_eq!(
            Relationship::new(nvidia, RelationshipType::IssuedBy, nvda, provenance()),
            Err(RelationshipError::InvalidEndpoints {
                relationship_type: RelationshipType::IssuedBy,
                subject: Category::Entity,
                object: Category::Instrument,
            })
        );
    }

    #[test]
    fn denominated_in_accepts_currency_or_asset() {
        let nvda: CanonicalId = InstrumentId::generate().into();
        let usd: CanonicalId = CurrencyId::generate().into();
        let usdc: CanonicalId = InstrumentId::generate().into();
        for unit in [usd, usdc] {
            assert!(
                Relationship::new(nvda, RelationshipType::DenominatedIn, unit, provenance())
                    .is_ok()
            );
        }
        let venue: CanonicalId = VenueId::generate().into();
        assert!(
            Relationship::new(nvda, RelationshipType::DenominatedIn, venue, provenance()).is_err()
        );
    }

    #[test]
    fn types_without_rules_are_not_storable() {
        let a: CanonicalId = InstrumentId::generate().into();
        let b: CanonicalId = InstrumentId::generate().into();
        assert!(Relationship::new(a, RelationshipType::DerivesFrom, b, provenance()).is_err());
        assert!(Relationship::new(a, RelationshipType::RelatedTo, b, provenance()).is_err());
    }

    #[test]
    fn self_reference_is_rejected() {
        let a: CanonicalId = InstrumentId::generate().into();
        assert_eq!(
            Relationship::new(a, RelationshipType::DenominatedIn, a, provenance()),
            Err(RelationshipError::SelfReference)
        );
    }
}
