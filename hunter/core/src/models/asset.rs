//! Known-mint asset classification for the wallet/portfolio surface.
//!
//! Cash (USDC) is dry powder — valued at face USD, excluded from trading PnL /
//! position counts / orphan reconcile. WSOL is expected AMM plumbing, not an
//! orphaned bag. Everything else is a meme (or other) trading position.

use serde::Serialize;

use crate::config::constants::{USDC_MINT, WSOL_MINT};

/// How the portfolio layer treats a held mint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Stable working capital (USDC). Face-valued; not a trading position.
    Cash,
    /// Transient wrap account for AMM swaps — expected non-position balance.
    WrappedSol,
    /// Default: meme / other SPL inventory the bot trades.
    Meme,
}

/// Classify a mint. Unknown mints are [`AssetKind::Meme`].
pub fn asset_kind(mint: &str) -> AssetKind {
    if mint == USDC_MINT {
        AssetKind::Cash
    } else if mint == WSOL_MINT {
        AssetKind::WrappedSol
    } else {
        AssetKind::Meme
    }
}

/// Cash / dry-powder mint (currently USDC only).
pub fn is_cash(mint: &str) -> bool {
    matches!(asset_kind(mint), AssetKind::Cash)
}

/// Balances that are expected in the wallet without an open strategy position
/// (WSOL plumbing + cash). Excluded from the boot orphan reconcile report.
pub fn is_expected_non_position(mint: &str) -> bool {
    matches!(asset_kind(mint), AssetKind::Cash | AssetKind::WrappedSol)
}

/// Human symbol for a known cash mint when enrichment is absent.
pub fn cash_symbol(mint: &str) -> Option<&'static str> {
    (mint == USDC_MINT).then_some("USDC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_mints() {
        assert_eq!(asset_kind(USDC_MINT), AssetKind::Cash);
        assert_eq!(asset_kind(WSOL_MINT), AssetKind::WrappedSol);
        assert_eq!(asset_kind("SomeMeme111"), AssetKind::Meme);
    }

    #[test]
    fn expected_non_position_covers_cash_and_wsol() {
        assert!(is_expected_non_position(USDC_MINT));
        assert!(is_expected_non_position(WSOL_MINT));
        assert!(!is_expected_non_position("SomeMeme111"));
    }
}
