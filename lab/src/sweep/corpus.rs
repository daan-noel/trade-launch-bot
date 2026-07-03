//! Corpus model: the in-memory set of per-token trade histories a sweep calls
//! `simulate` over many times without ever touching a data source in the loop.
//!
//! The [`CorpusSource`] trait has one impl — [`LakeSource`](crate::lake::duck) —
//! which reads the immutable day-partitioned Parquet lake through an in-memory
//! DuckDB (candidate select + per-mint `ROW_NUMBER` cap + fingerprints from the
//! `tokens` dimension). The old Postgres `DbSource` + Parquet corpus-cache path was
//! retired once the lake was proven to produce byte-identical metrics.
//!
//! The full `Trade` never enters the sweep loop: each token is projected **once at
//! load** into a slim, wallet-interned [`CorpusTrade`] buffer (see
//! [`crate::sweep::projection`]) — the hot loop walks that, not the ~250 B `Trade`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::models::trade::TradeRow;
use crate::sweep::grouping::TokenFingerprint;
use crate::sweep::projection::{project_trades, CorpusTrade};

/// One token's trade history, ready for `simulate`. `trades` is the slim
/// [`CorpusTrade`] projection (~3× smaller than `Trade`), shared (`Arc`) so building
/// a sub-corpus per group is a refcount clone, not a copy. Both the grouped sweep
/// and single-rule simulate load this one shape.
///
/// `fp` carries the token-creation fingerprint used only by the grouping layer
/// (never by `simulate`). The lake source fills it from the `tokens` dimension at
/// load (`has_fingerprints`), so the grouping layer can read it directly.
#[derive(Clone)]
pub struct CorpusToken {
    pub mint: String,
    pub symbol: String,
    pub trades: Arc<Vec<CorpusTrade>>,
    pub fp: TokenFingerprint,
}

impl CorpusToken {
    /// Project a token's chronological `Trade` slice into the slim buffer once, at
    /// load. Apply any corpus-wide trade filter (e.g. `curve_only`) **before**
    /// calling this — `CorpusTrade` drops `venue`.
    pub fn from_trades<T: TradeRow<Wallet = String>>(
        mint: String,
        symbol: String,
        fp: TokenFingerprint,
        trades: &[T],
    ) -> Self {
        Self {
            mint,
            symbol,
            fp,
            trades: Arc::new(project_trades(trades)),
        }
    }
}

/// The whole loaded population plus the hash that identifies it for the warm
/// in-memory cache.
pub struct Corpus {
    pub tokens: Vec<CorpusToken>,
    /// Stable hash naming the realised selection — keys the warm corpus cache.
    pub hash: String,
    /// True when token fingerprints were loaded with the trades (the lake source
    /// always embeds them). Callers skip any separate fingerprint pass when set.
    pub has_fingerprints: bool,
}

impl Corpus {
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn trade_count(&self) -> usize {
        self.tokens.iter().map(|t| t.trades.len()).sum()
    }
}

/// Which slice of a token's lifetime the per-mint cap keeps when the token has more
/// than `per_mint_cap` lifetime trades. Only matters for high-volume tokens past the
/// cap; below it the whole history loads either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TradeWindow {
    /// Keep the **earliest** `per_mint_cap` trades — the launch window the entry
    /// logic (`find_scalp_entry`) decides on. The correct default for a historical
    /// backtest: replaying newest-first would silently drop the first minutes and
    /// resolve entry against a truncated mid-life slice (Rec 4).
    #[default]
    LaunchWindow,
    /// Keep the **newest** `per_mint_cap` trades — parity with the live cache's
    /// retained recent window, for a "what would I do right now" replay. A
    /// selectable mode; the grouped-sweep handler defaults to `LaunchWindow`.
    #[allow(dead_code)]
    Recent,
}

/// Explicit population scope — never "load everything". A loader clips to
/// `token_cap` and/or `created_after`, logging which bound bit so the run never
/// silently truncates.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Explicit mint list; when `None`, the source picks the newest tokens.
    pub mints: Option<Vec<String>>,
    /// Hard cap on number of tokens loaded.
    pub token_cap: usize,
    /// Only tokens created at/after this instant.
    pub created_after: Option<DateTime<Utc>>,
    /// Only tokens created strictly before this instant — the upper bound of a
    /// date/time range. `None` ⇒ no upper bound.
    pub created_before: Option<DateTime<Utc>>,
    /// Per-mint trade cap for the batch query.
    pub per_mint_cap: i64,
    /// Which `per_mint_cap` slice to keep for over-cap tokens.
    pub window: TradeWindow,
    /// Drop AMM (post-migration) legs, keeping only bonding-curve trades.
    pub curve_only: bool,
    /// Populate each row's `tx_signature` from the lake (single-rule **simulate**
    /// renders `entry_tx`/`exit_tx` Solscan links). The sweep leaves this `false` so
    /// its rows stay slim — the trigger is resolved by index, not signature. Only
    /// affects the projected `CorpusTrade::tx_signature`; never changes which rows load
    /// or how they price.
    pub with_signatures: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            mints: None,
            token_cap: 5_000,
            created_after: None,
            created_before: None,
            per_mint_cap: crate::state::token_cache::MAX_TRADES_RETAINED as i64,
            window: TradeWindow::LaunchWindow,
            curve_only: false,
            with_signatures: false,
        }
    }
}

/// Default sweep per-mint trade cap (Phase 1.1). Launch-window scalp entries decide
/// on the first minutes, so a few thousand trades/token is plenty and cuts a
/// high-volume token's corpus weight (and its entry/exit walk) ~10–25×. Override per
/// box with `SWEEP_PER_MINT_CAP`.
pub const SWEEP_DEFAULT_PER_MINT_CAP: i64 = 5_000;

/// Effective per-mint trade cap for a sweep `Selection`: `SWEEP_PER_MINT_CAP` if set
/// (≥1), else [`SWEEP_DEFAULT_PER_MINT_CAP`]. This is the second factor of the corpus
/// weight (`tokens × trades/token`); `token_cap` bounds the first.
pub fn sweep_per_mint_cap() -> i64 {
    std::env::var("SWEEP_PER_MINT_CAP")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(SWEEP_DEFAULT_PER_MINT_CAP)
}

/// Source of a corpus — one interface, implemented by
/// [`LakeSource`](crate::lake::duck).
#[async_trait]
pub trait CorpusSource {
    async fn load(&self, sel: &Selection) -> Result<Corpus>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_per_mint_cap_defaults_when_env_unset() {
        // Reads the historical corpus, so this cap is NOT tied to the live in-RAM
        // MAX_TRADES_RETAINED cache (different storage tier). What must hold: a
        // positive default, returned by sweep_per_mint_cap() when the env override is
        // absent. (Env unset here because no test in this binary sets it.)
        assert!(SWEEP_DEFAULT_PER_MINT_CAP >= 1);
        std::env::remove_var("SWEEP_PER_MINT_CAP");
        assert_eq!(sweep_per_mint_cap(), SWEEP_DEFAULT_PER_MINT_CAP);
    }

    #[test]
    fn default_window_is_launch_window() {
        // A historical backtest must keep the launch prefix by default (Rec 4).
        assert_eq!(Selection::default().window, TradeWindow::LaunchWindow);
        assert_eq!(TradeWindow::default(), TradeWindow::LaunchWindow);
    }
}
