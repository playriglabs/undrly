//! Universe membership: "instrument X belongs to universe U as of T",
//! separate from "instrument X exists".
//!
//! Membership is snapshot metadata from a named source. It never affects
//! canonical identity: an instrument's id does not depend on its universes
//! or its rank in them.

use std::fmt;
use std::str::FromStr;

use crate::id::CanonicalId;
use crate::source::Provenance;
use crate::time::Timestamp;

/// The universes Undrly imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UniverseKey {
    /// Top 100 crypto assets by market capitalisation (CoinGecko).
    CryptoTop100,
    /// S&P 500, via the SPY ETF's holdings (not the official S&P file).
    Sp500,
    /// Nasdaq-100 (nasdaq.com), members with a trustworthy identifier.
    Nasdaq100,
    /// Hyperliquid's live perpetual markets.
    HyperliquidPerps,
}

impl UniverseKey {
    pub const ALL: [UniverseKey; 4] = [
        UniverseKey::CryptoTop100,
        UniverseKey::Sp500,
        UniverseKey::Nasdaq100,
        UniverseKey::HyperliquidPerps,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            UniverseKey::CryptoTop100 => "crypto-top100",
            UniverseKey::Sp500 => "sp500",
            UniverseKey::Nasdaq100 => "nasdaq100",
            UniverseKey::HyperliquidPerps => "hyperliquid-perps",
        }
    }
}

impl fmt::Display for UniverseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown universe `{0}`")]
pub struct UnknownUniverse(pub String);

impl FromStr for UniverseKey {
    type Err = UnknownUniverse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownUniverse(s.to_owned()))
    }
}

/// One member of a universe snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseMember {
    pub node: CanonicalId,
    /// The source's rank (e.g. market-cap rank), when it has one.
    pub rank: Option<u32>,
    /// The member's symbol as the universe source spells it.
    pub source_symbol: Option<String>,
}

/// A universe's membership as of `as_of`, asserted by one upstream record
/// (`provenance` names that record's source and receipt time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseSnapshot {
    pub key: UniverseKey,
    /// The source's own as-of time for the membership.
    pub as_of: Timestamp,
    pub members: Vec<UniverseMember>,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for k in UniverseKey::ALL {
            assert_eq!(k.as_str().parse::<UniverseKey>(), Ok(k));
        }
        assert!("sp400".parse::<UniverseKey>().is_err());
    }
}
