//! Strategy-dispatched **incremental exit** primitives for the live hot path.
//!
//! The kernel's [`resolve_exit`](super::registry::StrategyImpl::resolve_exit) is a
//! *batch* full re-walk (sweep/paper-replay). The live runner instead keeps a
//! **stateful** per-position memo so the per-second clock sweep and the per-trade
//! gate never re-walk a token's retained history each tick. That memo
//! ([`CachedExitState`](super::tpsl_sniper_1::exit::CachedExitState)) is a *distinct
//! type per strategy* — tpsl1's is plain, tpsl2's is generic over the wallet type
//! and carries an E5 launch-cohort memo — so this module enum-wraps both behind a
//! single API the unified [`StrategyRuntimeCache`](super::runtime_cache) can store
//! and dispatch by [`StrategyImpl`].
//!
//! Concrete to [`CachedTrade`] (the live token-cache feed is the only producer), so
//! no extra generics leak into the cache. The dispatch returns the **`&'static str`**
//! reason (the two `ExitReason` enums are also distinct), which is exactly what
//! [`StrategyPosition::exit_reason`] and [`kernel::ExitCode::from_reason`] consume.

use chrono::{DateTime, Utc};

use crate::models::StrategyPosition;
use crate::state::token_cache::CachedTrade;

use super::registry::{StrategyImpl, StrategyParams};
use super::{swing_1 as sw, tpsl_sniper_1 as t1, tpsl_sniper_2 as t2};

/// Strategy-dispatched exit-ladder params, resolved once per active rule at
/// [`set_rules`](super::runtime_cache::StrategyRuntimeCache::set_rules) time. The two
/// inner `LadderParams` are distinct types (tpsl2 adds the E5 cohort ratio), so this
/// enum-wraps them. Cheap to clone (all scalar fields).
#[derive(Clone)]
pub enum LadderParamsImpl {
    Tpsl1(t1::exit::LadderParams),
    Tpsl2(t2::exit::LadderParams),
    /// Boxed: swing1's ladder carries the swing-scan params + next-kill profile (much
    /// larger than the tpsl scalar ladders). The ladder is cloned per exit-event
    /// (`ladder_params_by_id`), so boxing keeps that clone cheap for every strategy.
    Swing1(Box<sw::exit::LadderParams>),
}

impl LadderParamsImpl {
    /// Resolve the ladder from parsed rule params (the typed
    /// [`StrategyParams`]). `to_rule()` rebuilds the rule the `from_rule`
    /// constructors expect — universal knobs are inert placeholders the ladder
    /// never reads, so this is decision-identical to the legacy edge.
    pub fn from_params(params: &StrategyParams) -> Self {
        match params {
            StrategyParams::Tpsl1(p) => {
                Self::Tpsl1(t1::exit::LadderParams::from_rule(&p.to_rule()))
            }
            StrategyParams::Tpsl2(p) => {
                Self::Tpsl2(t2::exit::LadderParams::from_rule(&p.to_rule()))
            }
            StrategyParams::Swing1(p) => {
                Self::Swing1(Box::new(sw::exit::LadderParams::from_rule(&p.to_rule())))
            }
        }
    }

    /// E2 TimeStop deadline (seconds since entry), if configured.
    pub fn time_stop_secs(&self) -> Option<u64> {
        match self {
            Self::Tpsl1(p) => p.time_stop_secs(),
            Self::Tpsl2(p) => p.time_stop_secs(),
            Self::Swing1(p) => p.time_stop_secs(),
        }
    }

    /// E3 Stall deadline (seconds since last higher-high), if configured.
    pub fn stall_secs(&self) -> Option<u64> {
        match self {
            Self::Tpsl1(p) => p.stall_secs(),
            Self::Tpsl2(p) => p.stall_secs(),
            Self::Swing1(p) => p.stall_secs(),
        }
    }

    /// Whether the rule carries any wall-clock exit (TimeStop or Stall) — the
    /// time-exit secondary-index membership gate.
    pub fn has_time_exit(&self) -> bool {
        self.time_stop_secs().is_some() || self.stall_secs().is_some()
    }

    /// The strategy this ladder dispatches to.
    pub fn strategy(&self) -> StrategyImpl {
        match self {
            Self::Tpsl1(_) => StrategyImpl::Tpsl1,
            Self::Tpsl2(_) => StrategyImpl::Tpsl2,
            Self::Swing1(_) => StrategyImpl::Swing1,
        }
    }
}

/// Strategy-dispatched per-position exit-walk memo (the running peaks + E5 cohort
/// net, advanced as trades print). Stored in the runtime cache keyed by position id.
pub enum CachedExitStateImpl {
    Tpsl1(t1::exit::CachedExitState),
    Tpsl2(t2::exit::CachedExitState<u32>),
    /// Boxed: the swing memo carries the swing-scan machine (`SwingParams` + a `Vec`
    /// of finalized legs), far larger than the tpsl variants — boxing keeps every
    /// per-position slot small (the memo is stored, not passed on the hot path).
    Swing1(Box<sw::exit::CachedSwingExitState>),
}

impl CachedExitStateImpl {
    /// Seed a **fully-folded** memo from the retained post-entry history (the
    /// clock-sweep's first-sight seed). tpsl2 seeds without the E5 cohort — the
    /// clock path only checks TimeStop/Stall; the cohort is lazily attached if a
    /// later trade ping needs E5 (see [`advance_and_find_exit`](Self::advance_and_find_exit)).
    pub fn build(
        trades: &[CachedTrade],
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        params: &LadderParamsImpl,
    ) -> Self {
        match params {
            LadderParamsImpl::Tpsl1(_) => {
                Self::Tpsl1(t1::exit::CachedExitState::build(trades, base, entry_price, entry_time))
            }
            LadderParamsImpl::Tpsl2(_) => {
                Self::Tpsl2(t2::exit::CachedExitState::build(trades, base, entry_price, entry_time))
            }
            LadderParamsImpl::Swing1(p) => Self::Swing1(Box::new(
                sw::exit::CachedSwingExitState::build(trades, base, entry_price, entry_time, p),
            )),
        }
    }

    /// Seed an **unfolded** memo whose cursor sits at the window front, so the first
    /// [`advance_and_find_exit`](Self::advance_and_find_exit) folds the entire
    /// retained window (reproducing a full re-walk on the position's first ping).
    /// tpsl2 seeds its E5 cohort here from the retained window when E5 is configured.
    pub fn build_unfolded(
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        trades: &[CachedTrade],
        params: &LadderParamsImpl,
    ) -> Self {
        match params {
            LadderParamsImpl::Tpsl1(_) => Self::Tpsl1(t1::exit::CachedExitState::build_unfolded(
                base,
                entry_price,
                entry_time,
            )),
            LadderParamsImpl::Tpsl2(p) => Self::Tpsl2(t2::exit::CachedExitState::build_unfolded(
                trades,
                base,
                entry_price,
                entry_time,
                p,
            )),
            LadderParamsImpl::Swing1(p) => Self::Swing1(Box::new(
                sw::exit::CachedSwingExitState::build_unfolded(base, entry_price, entry_time, p),
            )),
        }
    }

    /// Fold the genuinely-new trades into the memo **and** evaluate the exit ladder
    /// against them in one pass, returning the first exit reason that fires.
    /// Decision-equivalent to a full re-walk (see the per-strategy `CachedExitState`
    /// docs). For tpsl2 this lazily attaches the E5 cohort first (a clock-seeded memo
    /// has none), exactly mirroring the legacy tpsl2 runtime cache. A params/memo
    /// strategy mismatch is a no-op (`None`) — it never panics.
    pub fn advance_and_find_exit(
        &mut self,
        trades: &[CachedTrade],
        base: u64,
        params: &LadderParamsImpl,
    ) -> Option<&'static str> {
        match (self, params) {
            (Self::Tpsl1(c), LadderParamsImpl::Tpsl1(p)) => {
                c.advance_and_find_exit(trades, base, p).map(|r| r.as_str())
            }
            (Self::Tpsl2(c), LadderParamsImpl::Tpsl2(p)) => {
                c.ensure_cohort_seeded(trades, base, p);
                c.advance_and_find_exit(trades, base, p).map(|r| r.as_str())
            }
            (Self::Swing1(c), LadderParamsImpl::Swing1(p)) => {
                c.advance_and_find_exit(trades, base, p).map(|r| r.as_str())
            }
            _ => None,
        }
    }

    /// Wall-clock exit check against the memoized peaks (E3 Stall / E2 TimeStop),
    /// for the per-second sweep — never re-walks the trade history. A params/memo
    /// strategy mismatch returns `None`.
    pub fn clock_exit_reason(
        &self,
        entry_time: DateTime<Utc>,
        params: &LadderParamsImpl,
        now: DateTime<Utc>,
    ) -> Option<&'static str> {
        match (self, params) {
            (Self::Tpsl1(c), LadderParamsImpl::Tpsl1(p)) => {
                t1::exit::find_clock_driven_exit(&c.state, entry_time, p, now).map(|r| r.as_str())
            }
            (Self::Tpsl2(c), LadderParamsImpl::Tpsl2(p)) => {
                t2::exit::find_clock_driven_exit(&c.state, entry_time, p, now).map(|r| r.as_str())
            }
            (Self::Swing1(c), LadderParamsImpl::Swing1(p)) => {
                c.clock_exit_reason(entry_time, p, now).map(|r| r.as_str())
            }
            _ => None,
        }
    }
}

/// Strategy-agnostic guard shared by both live exit gates: a position is evaluable
/// only while in the holding index (Arming / BuySubmitted / Holding) **with** a
/// recorded entry. A `None`/0 entry price means the fill isn't indexed yet —
/// evaluating it would divide by ~0 and fire a phantom TakeProfit, flapping the
/// position ExitPending↔Holding. Returns the entry time once those hold. Mirrors the
/// legacy `tpsl_sniper_*::exit::clock_entry_time(&Position)` against the unified
/// [`StrategyPosition`].
pub fn clock_entry_time(position: &StrategyPosition) -> Option<DateTime<Utc>> {
    if !position.is_in_holding_index() || position.entry_price.is_none() {
        return None;
    }
    position.entry_time
}
