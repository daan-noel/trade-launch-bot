//! `TpslVariant` — the per-strategy seam for the unified TPSL engine
//! (Phase 5.1, step 3).
//!
//! Once the engine modules move into `tpsl_core` (steps 4–5) they become generic
//! over `V: TpslVariant`. Everything that differs between tpsl1 and tpsl2 is
//! reached through this trait:
//!   * the three concrete repos (table-level DB isolation is preserved — the
//!     associated types are the per-strategy `tpslN_*` repos behind the step-2
//!     [`repos`] traits),
//!   * the strategy tag (`"TPSL1"` / `"TPSL2"`),
//!   * the **scalp** divergence (tpsl2 only): the real-mode entry arming wait
//!     ([`TpslVariant::await_entry_arm`]) and — wired in step 5 — the entry-fill
//!     resolver and the cohort exit gate.
//!
//! Step 3 is additive: the trait + the `V1`/`V2` units are defined and verified
//! to compile, but no engine code is generic over `V` yet. The typed entry-fill
//! and exit-gate hooks are intentionally **not** here: they return `EntryFill` /
//! `ExitReason`, which still live inside the per-strategy `entry`/`exit` modules.
//! Referencing them now would invert the module dependency (`tpsl_core` must not
//! depend on `tpsl_sniper_*`); they move into `tpsl_core` in step 5, and the
//! typed hooks land alongside them. `USES_SCALP` marks that seam in the meantime.

#![allow(dead_code)] // Consumed by steps 4–5 (engine modules generic over V).

use async_trait::async_trait;
use sqlx::PgPool;

use super::repos;
use crate::models::TpslRule;
use crate::storage::repositories::trade_repo::TradeRepo;
use crate::storage::repositories::tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo;
use crate::storage::repositories::tpsl1_position_repo::Tpsl1PositionRepo;
use crate::storage::repositories::tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo;
use crate::storage::repositories::tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo;
use crate::storage::repositories::tpsl2_position_repo::Tpsl2PositionRepo;
use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;

/// The per-strategy seam. Implemented by the zero-sized [`V1`] / [`V2`] markers;
/// the engine is (in later steps) generic over `V: TpslVariant` and reaches all
/// per-strategy behavior through associated types, constants, and these hooks.
#[async_trait]
pub trait TpslVariant: Clone + Copy + Send + Sync + 'static {
    /// Real-position repo (`tpslN_real_positions`).
    type PositionRepo: repos::PositionRepo + Clone + 'static;
    /// Paper-trading repo (`tpslN_paper_test_run` + `tpslN_paper_positions`).
    type PaperRepo: repos::PaperRepo + Clone + 'static;
    /// Strategy-rule repo (`tpslN_strategy_rules`).
    type RuleRepo: repos::RuleRepo + Clone + 'static;

    /// The `strategy` column tag stamped on positions (`"TPSL1"` / `"TPSL2"`).
    const NAME: &'static str;

    /// Whether this variant gates entries/exits on scalp-continuation logic.
    /// `false` for tpsl1 (the engine uses the first-slot-block entry-fill
    /// resolver and skips the cohort exit); `true` for tpsl2. The typed
    /// entry-fill / exit-gate hooks that read this land in step 5.
    const USES_SCALP: bool = false;

    fn position_repo(pool: PgPool) -> Self::PositionRepo;
    fn paper_repo(pool: PgPool) -> Self::PaperRepo;
    fn rule_repo(pool: PgPool) -> Self::RuleRepo;

    /// Real-mode entry arming: wait for the scalp entry signal before sending a
    /// snipe buy. The default (no scalp) arms immediately, so tpsl1's live entry
    /// fires a buy at once — its current behavior. tpsl2 overrides this in step 5
    /// to poll the feed for the first trade where every configured scalp gate
    /// holds (today's `await_scalp_entry_signal`), sizing the window from `rule`.
    /// Returns `true` to proceed with the buy, `false` to drop the unentered
    /// position (no qualifying signal within the window).
    async fn await_entry_arm(_mint: &str, _rule: &TpslRule, _trade_repo: &TradeRepo) -> bool {
        true
    }
}

/// tpsl1 — TP/SL sniper, no scalp gating.
#[derive(Clone, Copy, Debug, Default)]
pub struct V1;

/// tpsl2 — TP/SL sniper with scalp-continuation entry/exit gating.
#[derive(Clone, Copy, Debug, Default)]
pub struct V2;

#[async_trait]
impl TpslVariant for V1 {
    type PositionRepo = Tpsl1PositionRepo;
    type PaperRepo = Tpsl1PaperTradingRepo;
    type RuleRepo = Tpsl1StrategyRuleRepo;

    const NAME: &'static str = "TPSL1";
    // USES_SCALP defaults to false — tpsl1 has no scalp logic.

    fn position_repo(pool: PgPool) -> Self::PositionRepo {
        Tpsl1PositionRepo::new(pool)
    }
    fn paper_repo(pool: PgPool) -> Self::PaperRepo {
        Tpsl1PaperTradingRepo::new(pool)
    }
    fn rule_repo(pool: PgPool) -> Self::RuleRepo {
        Tpsl1StrategyRuleRepo::new(pool)
    }
    // await_entry_arm: the default (arm immediately) is exactly tpsl1's behavior.
}

#[async_trait]
impl TpslVariant for V2 {
    type PositionRepo = Tpsl2PositionRepo;
    type PaperRepo = Tpsl2PaperTradingRepo;
    type RuleRepo = Tpsl2StrategyRuleRepo;

    const NAME: &'static str = "TPSL2";
    const USES_SCALP: bool = true;

    fn position_repo(pool: PgPool) -> Self::PositionRepo {
        Tpsl2PositionRepo::new(pool)
    }
    fn paper_repo(pool: PgPool) -> Self::PaperRepo {
        Tpsl2PaperTradingRepo::new(pool)
    }
    fn rule_repo(pool: PgPool) -> Self::RuleRepo {
        Tpsl2StrategyRuleRepo::new(pool)
    }

    // NOTE (step 5): override `await_entry_arm` to poll for the scalp entry
    // signal (today's `tpsl_sniper_2::execution::real::await_scalp_entry_signal`),
    // once that logic moves into `tpsl_core`. Left as the default no-op here so
    // step 3 stays additive and behavior-preserving (the engine still calls the
    // per-strategy code until step 5).
}
