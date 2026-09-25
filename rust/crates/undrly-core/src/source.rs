//! Data sources and their redistribution terms.

use std::fmt;
use std::str::FromStr;

use crate::reference::DisplayName;
use crate::time::Timestamp;

/// Stable identifier of a data source, e.g. `example-source`.
///
/// Lowercase ASCII letters and digits in hyphen-separated groups, at most 64
/// bytes. A source identifies *who supplied* a fact; it is deliberately not a
/// canonical `undrly:` identifier because sources are not market objects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(Box<str>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid source identifier `{0}`")]
pub struct SourceIdError(String);

impl SourceId {
    pub fn parse(s: &str) -> Result<Self, SourceIdError> {
        let valid = !s.is_empty()
            && s.len() <= 64
            && s.split('-').all(|group| {
                !group.is_empty()
                    && group
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        if valid {
            Ok(Self(s.into()))
        } else {
            Err(SourceIdError(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SourceId {
    type Err = SourceIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Whether Undrly may redistribute data obtained from a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Redistribution {
    /// Terms explicitly permit redistribution.
    Permitted,
    /// Terms restrict redistribution.
    Restricted,
    /// Terms have not been reviewed. Must be handled as [`Self::Restricted`].
    Unknown,
}

impl Redistribution {
    /// Only an explicit permission allows public exposure.
    pub fn allows_public_exposure(self) -> bool {
        matches!(self, Redistribution::Permitted)
    }
}

/// Who asserted a fact and when Undrly received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub source_id: SourceId,
    pub received_at: Timestamp,
}

/// A provider of facts or observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: SourceId,
    pub name: DisplayName,
    pub redistribution: Redistribution,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_slugs() {
        for s in ["example-source", "nasdaq", "source-2"] {
            assert_eq!(SourceId::parse(s).unwrap().as_str(), s);
        }
    }

    #[test]
    fn rejects_invalid_slugs() {
        let too_long = "a".repeat(65);
        for s in [
            "",
            "Example",
            "a--b",
            "-a",
            "a-",
            "a_b",
            "a b",
            "undrly:source:x",
            too_long.as_str(),
        ] {
            assert!(SourceId::parse(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn unknown_redistribution_is_not_public() {
        assert!(Redistribution::Permitted.allows_public_exposure());
        assert!(!Redistribution::Restricted.allows_public_exposure());
        assert!(!Redistribution::Unknown.allows_public_exposure());
    }
}
