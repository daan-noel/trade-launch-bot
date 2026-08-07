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
    /// Token creation time (from the `tokens` dimension) — the metric clock's
    /// origin. The generic engine's `time`/`stall` metrics + synthetic-tick grid
    /// are anchored here, so this MUST be the same `created_at` single-rule
    /// simulate feeds `ReplayToken`, or sweep and simulate would disagree on any
    /// `time`-based condition (the 5.3 parity keystone). The legacy tpsl/swing
    /// sweeps ignore it (they resolve entry/exit from trades alone).
    pub created_at: DateTime<Utc>,
    pub trades: Arc<Vec<CorpusTrade>>,
    pub fp: TokenFingerprint,
    /// `hunter_engine::token_identity_hash(name, symbol)` from the lake's tokens
    /// dimension — the copycat key. `None` = blank name/symbol, or a lake exported
    /// before the column existed (re-run `lake-export`). Only the duplicate-identity
    /// guard reads it; every other analysis path ignores it.
    pub identity: Option<u64>,
}

impl CorpusToken {
    /// Project a token's chronological `Trade` slice into the slim buffer once, at
    /// load. Apply any corpus-wide trade filter (e.g. `curve_only`) **before**
    /// calling this — `CorpusTrade` drops `venue`.
    pub fn from_trades<T: TradeRow<Wallet = String>>(
        mint: String,
        symbol: String,
        created_at: DateTime<Utc>,
        fp: TokenFingerprint,
        trades: &[T],
    ) -> Self {
        Self {
            mint,
            symbol,
            created_at,
            fp,
            trades: Arc::new(project_trades(trades)),
            // PG-sourced corpora carry no tokens-dimension row; the lake path fills
            // this in `attach_fingerprints`.
            identity: None,
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
    /// True when the lake **candidate** select hit `Selection::token_cap`
    /// (`ORDER BY created_at DESC LIMIT N` saturated). Independent of the final
    /// corpus size after `curve_only` / empty-history drops — those can leave
    /// `tokens.len() < token_cap` even when older mints were trimmed out.
    pub candidates_capped: bool,
}

impl Corpus {
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn trade_count(&self) -> usize {
        self.tokens.iter().map(|t| t.trades.len()).sum()
    }

    /// Block time of the **newest trade anywhere in the corpus** — how fresh the data
    /// this run scanned actually is. `None` for a trade-less corpus.
    ///
    /// One definition, two readers: the frozen-tail resolve anchors its corpus-wide
    /// horizon on it (`generic::strategy::frozen_tail_horizon`), and the run row stores
    /// it so the UI can say "data through HH:MM" — a sweep reads the sealed lake only,
    /// so a stale export is otherwise invisible next to a simulate that splices the
    /// fresh PG tail.
    ///
    /// Canonical per-mint order is block-time-monotone, so a token's last trade carries
    /// its own max — the corpus max is the max over those.
    pub fn last_trade_at(&self) -> Option<DateTime<Utc>> {
        Self::last_trade_at_of(&self.tokens)
    }

    /// [`Self::last_trade_at`] over a bare token slice (the sweep hands the strategy a
    /// slice, not the `Corpus`).
    pub fn last_trade_at_of(tokens: &[CorpusToken]) -> Option<DateTime<Utc>> {
        tokens.iter().filter_map(|t| t.trades.last().map(|ct| ct.block_time)).max()
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
    /// Populate each row's `ix_labels` + `wallet` for volume-flow metrics. Default
    /// `false` so non-flow sweeps pay zero extra RAM; set when a run's rules/axes
    /// reference `m_flow_*` (V2 wires this).
    pub with_flow: bool,
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
            with_flow: false,
        }
    }
}

/// Default sweep per-mint trade cap: **uncapped** (`i64::MAX` ⇒ the DuckDB `rn <= ?`
/// clip is a no-op, exactly like single-rule simulate's `SIM_PER_MINT_CAP`). Analysis
/// runs over each token's **entire** trade history — the grouped sweep and single-rule
/// simulate therefore price high-volume tokens (>launch-window) identically, closing
/// the old parity gap where the sweep truncated to the first ~5k trades but simulate
/// did not. `SWEEP_PER_MINT_CAP` (≥1) remains an **opt-in** bound for a workstation that
/// needs to cut corpus weight (`tokens × trades/token`) for a faster/lighter run.
pub const SWEEP_DEFAULT_PER_MINT_CAP: i64 = i64::MAX;

/// Effective per-mint trade cap for a sweep `Selection`: `SWEEP_PER_MINT_CAP` if set
/// (≥1), else [`SWEEP_DEFAULT_PER_MINT_CAP`] (uncapped — full history). `token_cap`
/// bounds the token count (selection scope); this bounds trades/token (opt-in only).
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
        // Analysis runs over full history: the default is uncapped (i64::MAX), NOT the
        // live in-RAM MAX_TRADES_RETAINED cache (different storage tier, different job).
        // What must hold: with no env override, sweep_per_mint_cap() returns the
        // uncapped default. (Env unset here because no test in this binary sets it.)
        assert_eq!(SWEEP_DEFAULT_PER_MINT_CAP, i64::MAX);
        std::env::remove_var("SWEEP_PER_MINT_CAP");
        assert_eq!(sweep_per_mint_cap(), SWEEP_DEFAULT_PER_MINT_CAP);
    }

    #[test]
    fn last_trade_at_is_the_corpus_wide_max() {
        use crate::models::trade::{Trade, TradeType};
        use chrono::Duration;

        let t0 = Utc::now() - Duration::seconds(1000);
        let mint_at = |mint: &str, offsets: &[i64]| {
            let trades: Vec<Trade> = offsets
                .iter()
                .map(|s| {
                    Trade::new(
                        mint.into(),
                        "w".into(),
                        TradeType::Buy,
                        1.0,
                        1,
                        format!("sig{s}"),
                        *s as u64,
                        t0 + Duration::seconds(*s),
                    )
                })
                .collect();
            CorpusToken::from_trades(
                mint.into(),
                mint.into(),
                t0,
                TokenFingerprint::default(),
                &trades,
            )
        };
        // The freshest trade is NOT on the last token, and the busiest token is not the
        // freshest — a max over per-token *last* trades, not a last-token read.
        let corpus = Corpus {
            tokens: vec![
                mint_at("a", &[0, 10, 20]),
                mint_at("b", &[5, 900]),
                mint_at("c", &[1, 2, 3, 4]),
            ],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        assert_eq!(corpus.last_trade_at(), Some(t0 + Duration::seconds(900)));
        // A trade-less corpus has no freshness to report — never a fabricated `now`.
        let empty = Corpus {
            tokens: vec![],
            hash: "h".into(),
            has_fingerprints: false,
            candidates_capped: false,
        };
        assert_eq!(empty.last_trade_at(), None);
    }

    #[test]
    fn default_window_is_launch_window() {
        // A historical backtest must keep the launch prefix by default (Rec 4).
        assert_eq!(Selection::default().window, TradeWindow::LaunchWindow);
        assert_eq!(TradeWindow::default(), TradeWindow::LaunchWindow);
    }
}
