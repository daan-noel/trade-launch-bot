//! Venue trait contracts — the seam that lets a launchpad plug in behind a stable
//! interface, so a NEW launchpad is a `launchpads` dimension row + a trait impl,
//! never a schema migration.
//!
//! platform-core defines only the CONTRACT (and it stays solana-free — addresses
//! are `&str`). The pump.fun impl lives on the live side (`ingest-host` /
//! `launcher`), where the borrowed decoders + on-chain types live.

use std::str::FromStr;

/// The kind of market a trade happened on. Mirrors the `market_kind` CHECK
/// constraint on `markets` / `trades` — the ONE place the string values are
/// defined in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketKind {
    BondingCurve,
    Amm,
    Clmm,
    Orderbook,
}

impl MarketKind {
    /// The exact DB string (must match the SQL CHECK constraint).
    pub fn as_str(self) -> &'static str {
        match self {
            MarketKind::BondingCurve => "bonding_curve",
            MarketKind::Amm => "amm",
            MarketKind::Clmm => "clmm",
            MarketKind::Orderbook => "orderbook",
        }
    }
}

impl FromStr for MarketKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bonding_curve" => Ok(MarketKind::BondingCurve),
            "amm" => Ok(MarketKind::Amm),
            "clmm" => Ok(MarketKind::Clmm),
            "orderbook" => Ok(MarketKind::Orderbook),
            other => Err(format!("unknown market_kind: {other}")),
        }
    }
}

/// A launchpad adapter: the minimal identity/classification contract the
/// ingest and launcher hosts implement per platform. Decoding of raw txs stays
/// in the live-side transport (`ingest-laserstream`); this trait resolves
/// identity so rows land with the right interned dimensions.
pub trait LaunchpadAdapter: Send + Sync {
    /// The `launchpads.key` this adapter serves (e.g. `"pump_fun"`).
    fn key(&self) -> &str;

    /// The interned `launchpads.id` (stamped denormalized onto hot rows).
    fn launchpad_id(&self) -> i16;

    /// The interned `quote_assets.id` a market trades against (e.g. SOL = 1).
    fn quote_asset_id(&self, kind: MarketKind) -> i16;

    /// Classify an on-chain program id to a `MarketKind`, or `None` if this
    /// adapter doesn't own that program.
    fn classify_market(&self, program_id: &str) -> Option<MarketKind>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_kind_str_roundtrips_and_matches_check() {
        for k in [
            MarketKind::BondingCurve,
            MarketKind::Amm,
            MarketKind::Clmm,
            MarketKind::Orderbook,
        ] {
            assert_eq!(MarketKind::from_str(k.as_str()).unwrap(), k);
        }
        // Exact strings the SQL CHECK constraint allows.
        assert_eq!(MarketKind::BondingCurve.as_str(), "bonding_curve");
        assert!(MarketKind::from_str("perp").is_err());
    }
}
