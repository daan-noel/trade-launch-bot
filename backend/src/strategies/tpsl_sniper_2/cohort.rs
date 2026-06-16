//! Shared launch-cohort primitive for the tpsl_sniper_2 scalp gates.
//!
//! The **cohort** is the launch sniper / bundler cluster: every wallet that
//! bought within `RUGGED_EARLY_SLOT_WINDOW` slots of a token's first trade.
//! Reusing the rug-detection window keeps **one cluster definition** across rug
//! detection, the entry gates (cohort-held / organic demand / organic liquidity)
//! and the E5 cohort-dump exit — so they can never drift apart.
//!
//! Everyone *not* in the cohort is "outside" (`O`): later/real buyers — the
//! continuation demand and the liquidity you exit into.

use std::collections::HashSet;

use crate::models::trade::TradeRow;

/// Wallets that bought within `slot_window` slots of the token's first trade —
/// the launch cohort. Empty when `trades` is empty. `slot_window` mirrors
/// [`crate::config::constants::RUGGED_EARLY_SLOT_WINDOW`].
///
/// Generic over [`TradeRow`] so the live path (`T = Trade`, `Wallet = String`)
/// and the sweep (`Wallet = u32`) share one cohort definition; the returned set
/// is keyed by the row's wallet identity.
pub fn early_cohort_wallets<T: TradeRow>(trades: &[T], slot_window: i64) -> HashSet<T::Wallet> {
    let Some(first) = trades.first() else {
        return HashSet::new();
    };
    let cutoff = first.slot().saturating_add(slot_window.max(0) as u64);
    trades
        .iter()
        .filter(|t| t.is_buy() && t.slot() <= cutoff)
        .map(|t| t.wallet().clone())
        .collect()
}

/// Token- and SOL-flow totals for a wallet set over a trade slice.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CohortFlow {
    /// Σ token_amount over the set's buys (everything it ever bought).
    pub bought_tokens: f64,
    /// Σ (buy − sell) token_amount — net tokens the set still holds.
    pub net_tokens: f64,
    /// Σ (buy − sell) sol_amount — net SOL the set put into the pool (its
    /// contribution to real reserves; dev seed + cohort deposits).
    pub net_sol: f64,
}

impl CohortFlow {
    /// Held ratio: net tokens still held / everything bought. `1.0` = holding the
    /// full bag, `→0` = sold out. `0.0` when the set bought nothing (no overhang
    /// to dump). Matches the rug-detection `cohort_net / cohort_bought` shape.
    ///
    /// Used by [`super::entry::scalp::scalp_features`] (the per-prefix feature
    /// oracle); the linearized `find_scalp_entry` computes the ratio inline.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn held_ratio(&self) -> f64 {
        if self.bought_tokens > 0.0 {
            self.net_tokens.max(0.0) / self.bought_tokens
        } else {
            0.0
        }
    }
}

/// Sum token/SOL flow for `wallets` over `trades`.
pub fn cohort_flow<T: TradeRow>(trades: &[T], wallets: &HashSet<T::Wallet>) -> CohortFlow {
    let mut f = CohortFlow::default();
    for t in trades.iter().filter(|t| wallets.contains(t.wallet())) {
        if t.is_buy() {
            f.bought_tokens += t.token_amount();
            f.net_tokens += t.token_amount();
            f.net_sol += t.sol_amount();
        } else {
            f.net_tokens -= t.token_amount();
            f.net_sol -= t.sol_amount();
        }
    }
    f
}

/// Net SOL flow for wallets **outside** the cohort (real demand from later/real
/// buyers): Σ (buy − sell) sol_amount over non-cohort wallets.
///
/// Used by [`super::entry::scalp::scalp_features`] (the per-prefix feature oracle);
/// the linearized `find_scalp_entry` carries the outside-net SOL running total.
#[cfg_attr(not(test), allow(dead_code))]
pub fn outside_net_sol<T: TradeRow>(trades: &[T], cohort: &HashSet<T::Wallet>) -> f64 {
    let mut net = 0.0;
    for t in trades.iter().filter(|t| !cohort.contains(t.wallet())) {
        if t.is_buy() {
            net += t.sol_amount();
        } else {
            net -= t.sol_amount();
        }
    }
    net
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::{DateTime, Utc};

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn trade(wallet: &str, side: TradeType, sol: f64, tokens: f64, slot: u64) -> Trade {
        Trade::new(
            "mint".into(),
            wallet.into(),
            side,
            sol,
            tokens,
            format!("sig-{wallet}-{slot}"),
            slot,
            base_time(),
        )
    }

    #[test]
    fn cohort_is_wallets_buying_within_the_slot_window() {
        let trades = vec![
            trade("dev", TradeType::Buy, 1.0, 100.0, 10),    // first slot = 10
            trade("early", TradeType::Buy, 1.0, 100.0, 50),  // within 10+150
            trade("late", TradeType::Buy, 1.0, 100.0, 500),  // outside the window
        ];
        let cohort = early_cohort_wallets(&trades, 150);
        assert!(cohort.contains("dev"));
        assert!(cohort.contains("early"));
        assert!(!cohort.contains("late"));
    }

    #[test]
    fn only_buys_join_the_cohort() {
        // A wallet whose first appearance in-window is a sell is not a launch buyer.
        let trades = vec![
            trade("dev", TradeType::Buy, 1.0, 100.0, 10),
            trade("seller", TradeType::Sell, 1.0, 100.0, 20),
        ];
        let cohort = early_cohort_wallets(&trades, 150);
        assert!(!cohort.contains("seller"));
    }

    #[test]
    fn held_ratio_tracks_net_over_bought() {
        let cohort: HashSet<String> = ["dev".to_string()].into_iter().collect();
        // Bought 100, sold 95 → net 5 / 100 = 0.05.
        let trades = vec![
            trade("dev", TradeType::Buy, 1.0, 100.0, 10),
            trade("dev", TradeType::Sell, 0.9, 95.0, 30),
        ];
        let flow = cohort_flow(&trades, &cohort);
        assert!((flow.bought_tokens - 100.0).abs() < 1e-9);
        assert!((flow.net_tokens - 5.0).abs() < 1e-9);
        assert!((flow.held_ratio() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn outside_net_sol_excludes_cohort() {
        let trades = vec![
            trade("dev", TradeType::Buy, 5.0, 100.0, 10),
            trade("outsider", TradeType::Buy, 3.0, 50.0, 200),
            trade("outsider", TradeType::Sell, 1.0, 10.0, 300),
        ];
        let cohort = early_cohort_wallets(&trades, 150); // only "dev"
        // Outside net = 3.0 buy - 1.0 sell = 2.0.
        assert!((outside_net_sol(&trades, &cohort) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn empty_trades_yield_empty_cohort_and_zero_flow() {
        let empty: [Trade; 0] = [];
        let cohort = early_cohort_wallets(&empty, 150);
        assert!(cohort.is_empty());
        assert_eq!(cohort_flow(&empty, &cohort), CohortFlow::default());
        assert_eq!(CohortFlow::default().held_ratio(), 0.0);
    }
}
