//! swing1 exit ladder — symmetric to entry: the dev starting another kill is the
//! signal to flee. Ladder priority (first match on a trade wins):
//!
//!   NextKill → E4 LiquidityExit (real reserves) → StopLoss → TakeProfit →
//!   E1 TrailingStop → E3 Stall → E2 TimeStop.
//!
//! NextKill is the top arm and is the swing1-specific addition; the rest reuse
//! the same predicate shapes as tpsl1/tpsl2 (E4 uses **real** reserves like
//! tpsl2). Every feature is inert at `None`/`0`.
//!
//! NextKill detection is **causal**: a single forward scan over the post-entry
//! trades finds the first swing-LOW leg matching the next-kill profile (deep +
//! short); the position flees at the first trade at/after that leg's terminal
//! pivot. One scan up front → O(1) check per trade in the walk, so the batch
//! result is reproducible by the live-incremental machine (Phase 2).

use chrono::{DateTime, Duration, Utc};

use crate::config::constants::MAX_FILL_WAIT_SLOTS;
use crate::models::trade::TradeRow;
use crate::models::Swing1Rule;

use super::classifier::LowFeatures;
use super::swing::{detect_swing_legs_raw, SwingType};
use super::{exit_next_kill_profile, swing_params_from_rule};

/// The typed reason a swing1 position exited. swing1-specific (intentional clone
/// of the tpsl ladders' reasons) so the **NextKill** top arm has a home; the
/// reason strings match the kernel's [`ExitCode::from_reason`] wire form.
///
/// [`ExitCode::from_reason`]: crate::strategies::kernel::ExitCode::from_reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// Top priority — the dev started another kill leg (deep + short post-entry low).
    NextKill,
    TakeProfit,
    StopLoss,
    TrailingStop,
    Stall,
    TimeStop,
    LiquidityExit,
}

impl ExitReason {
    /// Stable wire/string form — must match the kernel's `from_reason` arms.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NextKill => "NextKill",
            Self::TakeProfit => "TakeProfit",
            Self::StopLoss => "StopLoss",
            Self::TrailingStop => "TrailingStop",
            Self::Stall => "Stall",
            Self::TimeStop => "TimeStop",
            Self::LiquidityExit => "LiquidityExit",
        }
    }
}

/// The resolved exit fill (price/tx/time + reason).
#[derive(Debug, Clone, PartialEq)]
pub struct ExitFill {
    pub price: f64,
    pub tx_signature: String,
    pub block_time: DateTime<Utc>,
    pub reason: ExitReason,
}

/// The swing1 exit-ladder params, resolved once from a [`Swing1Rule`] so the
/// per-trade predicate is a pure function of state + trade + params.
struct LadderParams {
    take_profit_pct: f64,
    stop_loss_pct: f64,
    trailing_stop_pct: Option<f64>, // E1
    time_stop_secs: Option<u64>,    // E2
    stall_secs: Option<u64>,        // E3
    liquidity_drop_pct: Option<f64>, // E4 (real reserves)
}

impl LadderParams {
    fn from_rule(r: &Swing1Rule) -> Self {
        let nz_f64 = |v: Option<f64>| v.filter(|x| *x != 0.0);
        let nz_u64 = |v: Option<u64>| v.filter(|x| *x != 0);
        Self {
            take_profit_pct: r.p_exit_take_profit,
            stop_loss_pct: r.p_exit_stop_loss,
            trailing_stop_pct: nz_f64(r.p_exit_trailing_stop_pct),
            time_stop_secs: nz_u64(r.p_exit_time_stop_secs),
            stall_secs: nz_u64(r.p_exit_stall_secs),
            liquidity_drop_pct: nz_f64(r.p_exit_liquidity_drop_pct),
        }
    }
}

/// Running peaks accumulated while walking post-entry trades.
struct WalkState {
    peak_price: f64,
    last_higher_high_time: DateTime<Utc>,
    peak_reserves: f64,
}

impl WalkState {
    fn starting_at(entry_price: f64, entry_time: DateTime<Utc>) -> Self {
        Self {
            peak_price: entry_price,
            last_higher_high_time: entry_time,
            peak_reserves: 0.0,
        }
    }

    fn update_with_trade<T: TradeRow>(&mut self, t: &T) {
        let price = t.price_per_token();
        if price > self.peak_price {
            self.peak_price = price;
            self.last_higher_high_time = t.block_time();
        }
        // E4 tracks **real** reserves (the tpsl2 variant).
        if let Some(r) = t.real_sol_reserves() {
            if r > self.peak_reserves {
                self.peak_reserves = r;
            }
        }
    }
}

/// The first post-entry timestamp (ms) at which a next-kill leg completes, or
/// `None` if next-kill is disabled / never fires. Causal single scan.
fn next_kill_fire_ms<T: TradeRow>(trades: &[T], rule: &Swing1Rule, entry_time: DateTime<Utc>) -> Option<i64> {
    let profile = exit_next_kill_profile(rule)?;
    let sparams = swing_params_from_rule(rule);
    // Only legs whose terminal pivot lands after entry can be a *next* kill.
    let entry_ms = entry_time.timestamp_millis();
    let legs = detect_swing_legs_raw(trades, &sparams);
    for leg in &legs {
        if leg.leg_type != SwingType::SwingLow || leg.end_at <= entry_ms {
            continue;
        }
        let f = LowFeatures::from_low(leg).expect("SwingLow checked");
        if profile.is_kill_low(&f) {
            return Some(leg.end_at.max(entry_ms + 1));
        }
    }
    None
}

/// The exit ladder for one trade. First feature that fires wins.
fn ladder_reason<T: TradeRow>(
    state: &WalkState,
    t: &T,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    params: &LadderParams,
    next_kill_ms: Option<i64>,
) -> Option<ExitReason> {
    let price = t.price_per_token();
    let block_time = t.block_time();
    let pct = ((price - entry_price) / entry_price) * 100.0;
    None
        .or_else(|| {
            // NextKill — top priority: the dev starting another kill.
            next_kill_ms
                .filter(|&ms| block_time.timestamp_millis() >= ms)
                .map(|_| ExitReason::NextKill)
        })
        .or_else(|| {
            // E4: real reserves crash below the peak-since-entry.
            params.liquidity_drop_pct.and_then(|drop| {
                t.real_sol_reserves().and_then(|reserves| {
                    (state.peak_reserves > 0.0
                        && reserves < state.peak_reserves * (1.0 - drop / 100.0))
                    .then_some(ExitReason::LiquidityExit)
                })
            })
        })
        .or_else(|| (pct <= -params.stop_loss_pct).then_some(ExitReason::StopLoss))
        .or_else(|| (pct >= params.take_profit_pct).then_some(ExitReason::TakeProfit))
        .or_else(|| {
            params.trailing_stop_pct.and_then(|trail| {
                (state.peak_price > 0.0 && price <= state.peak_price * (1.0 - trail / 100.0))
                    .then_some(ExitReason::TrailingStop)
            })
        })
        .or_else(|| {
            params.stall_secs.and_then(|secs| {
                ((block_time - state.last_higher_high_time).num_seconds() >= secs as i64)
                    .then_some(ExitReason::Stall)
            })
        })
        .or_else(|| {
            params.time_stop_secs.and_then(|secs| {
                (block_time >= entry_time + Duration::seconds(secs as i64))
                    .then_some(ExitReason::TimeStop)
            })
        })
}

/// Walk the post-entry trades and return the first exit the ladder fires, or
/// `None` if still open. Mirrors tpsl1's worst-case fill window.
pub fn find_trade_driven_exit<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Swing1Rule,
) -> Option<ExitFill> {
    find_trade_driven_exit_with_slot(trades, entry_time, entry_price, rule).map(|(f, _)| f)
}

/// [`find_trade_driven_exit`] that also returns the **firing slot** (for the live
/// paper fill-poll window), matching the tpsl2 resolver shape.
pub fn find_trade_driven_exit_with_slot<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Swing1Rule,
) -> Option<(ExitFill, u64)> {
    if entry_price <= 0.0 {
        return None;
    }
    let params = LadderParams::from_rule(rule);
    let next_kill_ms = next_kill_fire_ms(trades, rule, entry_time);
    let mut state = WalkState::starting_at(entry_price, entry_time);

    for (fire_idx, t) in trades.iter().enumerate().filter(|(_, t)| t.block_time() > entry_time) {
        state.update_with_trade(t);

        let Some(reason) = ladder_reason(&state, t, entry_time, entry_price, &params, next_kill_ms)
        else {
            continue;
        };

        // Worst-case fill window: trigger slot F + the next observed slot within
        // MAX_FILL_WAIT_SLOTS; fill = lowest price in the window (worst for a sell).
        let exit_slot = t.slot();
        let post = &trades[fire_idx + 1..];
        let next_slot = post
            .iter()
            .filter(|x| x.block_time() > entry_time && x.slot() > exit_slot)
            .map(|x| x.slot())
            .next();
        let exit_trade = post
            .iter()
            .filter(|x| {
                x.block_time() > entry_time && {
                    let s = x.slot();
                    s == exit_slot
                        || next_slot.is_some_and(|ns| s == ns && ns <= exit_slot + MAX_FILL_WAIT_SLOTS)
                }
            })
            .min_by(|a, b| a.price_per_token().total_cmp(&b.price_per_token()));

        if let Some(et) = exit_trade {
            return Some((
                ExitFill {
                    price: et.price_per_token(),
                    tx_signature: et.tx_signature().to_string(),
                    block_time: et.block_time(),
                    reason,
                },
                exit_slot,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;

    fn base() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn tr(kind: TradeType, secs: i64, price: f64, slot: u64) -> Trade {
        // price = sol/token; set token_amount=1 so price_per_token == sol_amount.
        Trade::new(
            "mint".into(),
            "w".into(),
            kind,
            price,
            1,
            format!("sig-{secs}-{slot}"),
            slot,
            base() + Duration::seconds(secs),
        )
    }

    fn rule_tp_sl(tp: f64, sl: f64) -> Swing1Rule {
        let mut r = Swing1Rule::new(
            "r".into(), None, None, None, serde_json::json!([]), "paper".into(),
            1.0, tp, sl, None, None, None, None, None, None, None, None, None,
        );
        r.is_active = true;
        r
    }

    #[test]
    fn take_profit_fires_without_next_kill() {
        let rule = rule_tp_sl(50.0, 90.0);
        // entry @1.0; price jumps to 2.0 (>+50%) at slot 3 (the TP trigger). The
        // fill window needs a trade AFTER the trigger in the same/next slot — add a
        // slot-4 trade so the worst-case fill resolves.
        let trades = vec![
            tr(TradeType::Buy, 1, 1.0, 2),
            tr(TradeType::Buy, 2, 2.0, 3), // TP trigger
            tr(TradeType::Buy, 3, 1.9, 4), // fill window
        ];
        let f = find_trade_driven_exit(&trades, base(), 1.0, &rule).expect("exit");
        assert_eq!(f.reason, ExitReason::TakeProfit);
    }

    #[test]
    fn next_kill_fires_on_post_entry_kill_leg() {
        // Configure a next-kill profile (≥50% drop, ≤5s). With TP/SL out of reach,
        // a deep+short post-entry low must flee via NextKill.
        let mut rule = rule_tp_sl(500.0, 95.0); // TP/SL never hit by the path below
        rule.p_exit_next_kill_depth_min_pct = Some(0.5);
        rule.p_exit_next_kill_max_duration_ms = Some(5_000);
        // Reversal thresholds small so the swing scan splits legs.
        rule.p_swing_high_to_low_sol = Some(0.5);
        rule.p_swing_low_to_high_sol = Some(0.5);
        rule.p_swing_min_leg_trades = Some(1);
        // entry @1.0; pump to 3.0, then a deep fast dump to 1.2 (-60% off the 3.0
        // peak within ~2s → kill low), then a reversal that completes the low leg
        // and provides the flee fill.
        let trades = vec![
            tr(TradeType::Buy, 1, 1.0, 2),
            tr(TradeType::Buy, 2, 3.0, 3),  // up-leg
            tr(TradeType::Sell, 4, 1.2, 4), // deep dump within ~2s → kill low
            tr(TradeType::Buy, 6, 1.3, 5),  // reversal up → completes the low leg
            tr(TradeType::Buy, 7, 1.3, 6),  // flee fill window
        ];
        let f = find_trade_driven_exit(&trades, base(), 1.0, &rule).expect("exit");
        assert_eq!(f.reason, ExitReason::NextKill);
    }
}
