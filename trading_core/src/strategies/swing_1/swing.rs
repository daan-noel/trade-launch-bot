//! Token Swing Analyzer (generic over [`TradeRow`]).
//!
//! Detects alternating buy-dominant (swing high) and sell-dominant (swing low)
//! legs in a token's trade history. Moved here from `lab/src/analyzers/swing_analyzer.rs`
//! (swing1 Phase 1a) and made generic over `T: TradeRow` so the SAME analyzer
//! runs in the lab swing endpoint (`Trade`), the backtest sweep (`SweepTrade`),
//! and — later — live (`CachedTrade`). Pricing is the single shared GMGN spot
//! ([`TradeRow::chart_spot_price`]), so a leg detected offline is the leg
//! detected live.
//!
//! See `@project_plans/token_swing_analyzer_spec_using_TA_terms.md` for the full
//! specification this implementation follows.

use serde::{Deserialize, Serialize};

use crate::models::trade::TradeRow;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// Tunable parameters for swing detection. All fields are optional in the JSON
/// body and fall back to the defaults below.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwingParams {
    // Reversal thresholds: Swing High -> Swing Low (0 = no bound)
    #[serde(default = "default_high_to_low_sol")]
    pub high_to_low_threshold_sol: f64,
    #[serde(default = "default_pct")]
    pub high_to_low_threshold_pct: f64, // 0-100 (real percent), 0 = no bound

    // Reversal thresholds: Swing Low -> Swing High (0 = no bound)
    #[serde(default = "default_low_to_high_sol")]
    pub low_to_high_threshold_sol: f64,
    #[serde(default = "default_pct")]
    pub low_to_high_threshold_pct: f64, // 0-100 (real percent), 0 = no bound

    // Leg quality filters (min: 0 disables volume/duration/net_flow; max: 0 = no cap)
    #[serde(default = "default_min_leg_trades")]
    pub min_leg_trades: u32,
    #[serde(default)]
    pub min_leg_duration_ms: i64,
    #[serde(default)]
    pub min_leg_volume: f64,
    #[serde(default)]
    pub min_leg_net_flow: f64,
    #[serde(default)]
    pub max_leg_trades: u32,
    #[serde(default)]
    pub max_leg_duration_ms: i64,
    #[serde(default)]
    pub max_leg_volume: f64,
    #[serde(default)]
    pub max_leg_net_flow: f64,

    // Per-leg-type delta % and net-flow-per-second bounds, compared by MAGNITUDE
    // (absolute value) so swing lows — whose delta % and net flow are negative —
    // use the same positive thresholds as swing highs. 0 = no bound.
    // Delta % magnitude: |(end_price - start_price)/start_price*100|.
    // Net flow per second: |net_flow / (duration_ms/1000)|; skipped for 0-duration legs.
    #[serde(default)]
    pub swing_high_min_delta_pct: f64,
    #[serde(default)]
    pub swing_high_max_delta_pct: f64,
    #[serde(default)]
    pub swing_high_min_net_flow_per_sec: f64,
    #[serde(default)]
    pub swing_high_max_net_flow_per_sec: f64,
    #[serde(default)]
    pub swing_low_min_delta_pct: f64,
    #[serde(default)]
    pub swing_low_max_delta_pct: f64,
    #[serde(default)]
    pub swing_low_min_net_flow_per_sec: f64,
    #[serde(default)]
    pub swing_low_max_net_flow_per_sec: f64,

    // "Big transaction" threshold (SOL). 0 = disabled. When > 0, a single tx with
    // `sol_amount >= big_tx_sol` (a) confirms a reversal immediately, regardless of
    // the net-flow reversal threshold, and (b) anchors a leg's terminal pivot
    // (`pivot_end_*`) to the LAST such same-side tx — the real pump/dump point —
    // instead of the chronologically last (possibly dust) trade.
    #[serde(default)]
    pub big_tx_sol: f64,
}

fn default_high_to_low_sol() -> f64 {
    5.0
}
fn default_low_to_high_sol() -> f64 {
    5.0
}
fn default_pct() -> f64 {
    50.0
}
fn default_min_leg_trades() -> u32 {
    2
}

impl Default for SwingParams {
    fn default() -> Self {
        Self {
            high_to_low_threshold_sol: default_high_to_low_sol(),
            high_to_low_threshold_pct: default_pct(),
            low_to_high_threshold_sol: default_low_to_high_sol(),
            low_to_high_threshold_pct: default_pct(),
            min_leg_trades: default_min_leg_trades(),
            min_leg_duration_ms: 0,
            min_leg_volume: 0.0,
            min_leg_net_flow: 0.0,
            max_leg_trades: 0,
            max_leg_duration_ms: 0,
            max_leg_volume: 0.0,
            max_leg_net_flow: 0.0,
            swing_high_min_delta_pct: 0.0,
            swing_high_max_delta_pct: 0.0,
            swing_high_min_net_flow_per_sec: 0.0,
            swing_high_max_net_flow_per_sec: 0.0,
            swing_low_min_delta_pct: 0.0,
            swing_low_max_delta_pct: 0.0,
            swing_low_min_net_flow_per_sec: 0.0,
            swing_low_max_net_flow_per_sec: 0.0,
            big_tx_sol: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SwingType {
    SwingHigh,
    SwingLow,
}

/// A finalized swing leg, ready for serialization.
#[derive(Debug, Clone, Serialize)]
pub struct SwingLeg {
    #[serde(rename = "type")]
    pub leg_type: SwingType,
    pub start_at: i64,
    pub end_at: i64,
    pub duration_ms: i64,
    pub start_price: f64,
    pub end_price: f64,
    /// Terminal pivot for charting: timestamp/price of the leg's last "big" same-side
    /// tx (`sol_amount >= big_tx_sol`), falling back to the leg's price extreme
    /// (max spot for highs, min spot for lows) when the leg has no big tx. Distinct
    /// from `end_*`, which stays the full-leg span used by stats/filters.
    pub pivot_end_at: i64,
    pub pivot_end_price: f64,
    pub inflow: f64,
    pub outflow: f64,
    pub net_flow: f64,
    pub trade_count: u32,
}

// ---------------------------------------------------------------------------
// Internal representation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Buy,
    Sell,
}

pub(crate) struct Tx {
    timestamp_ms: i64,
    side: Side,
    sol_amount: f64,
    /// Post-trade GMGN spot (`reserve_sol / reserve_token` → pool → execution),
    /// used for `end_price`.
    price: f64,
    /// GMGN spot immediately BEFORE this trade (the previous trade's post-trade
    /// spot, since the curve is unchanged between trades). Used for `start_price`.
    pre_spot: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    SwingHigh,
    TempSwingLow,
    SwingLow,
    TempSwingHigh,
}

/// Mutable leg accumulator used during the scan.
#[derive(Clone)]
pub(crate) struct LegAcc {
    leg_type: SwingType,
    start_at: i64,
    end_at: i64,
    start_price: f64,
    end_price: f64,
    inflow: f64,
    outflow: f64,
    trade_count: u32,
    // Terminal-pivot tracking (does not affect stats/filters). `last_big_*` is the
    // last same-side tx with `sol_amount >= big_tx_sol`; `extreme_*` is the price
    // extreme (max spot for highs, min spot for lows) used as the fallback when the
    // leg contains no big tx.
    last_big_at: Option<i64>,
    last_big_price: Option<f64>,
    extreme_at: i64,
    extreme_price: f64,
}

impl LegAcc {
    /// Create a swing-high leg seeded from a BUY (fully consumes the tx).
    fn seed_high(tx: &Tx, big_tx_sol: f64) -> Self {
        let mut leg = Self {
            leg_type: SwingType::SwingHigh,
            start_at: tx.timestamp_ms,
            end_at: tx.timestamp_ms,
            start_price: tx.pre_spot,
            end_price: tx.price,
            inflow: tx.sol_amount,
            outflow: 0.0,
            trade_count: 1,
            last_big_at: None,
            last_big_price: None,
            extreme_at: tx.timestamp_ms,
            extreme_price: tx.price,
        };
        leg.consider_pivot(tx, big_tx_sol);
        leg
    }

    /// Create a swing-low leg seeded from a SELL (fully consumes the tx).
    fn seed_low(tx: &Tx, big_tx_sol: f64) -> Self {
        let mut leg = Self {
            leg_type: SwingType::SwingLow,
            start_at: tx.timestamp_ms,
            end_at: tx.timestamp_ms,
            start_price: tx.pre_spot,
            end_price: tx.price,
            inflow: 0.0,
            outflow: tx.sol_amount,
            trade_count: 1,
            last_big_at: None,
            last_big_price: None,
            extreme_at: tx.timestamp_ms,
            extreme_price: tx.price,
        };
        leg.consider_pivot(tx, big_tx_sol);
        leg
    }

    fn net_flow(&self) -> f64 {
        self.inflow - self.outflow
    }

    /// Update the terminal-pivot trackers from a same-side tx: advance the price
    /// extreme, and record this tx as the last big one when it clears `big_tx_sol`.
    fn consider_pivot(&mut self, tx: &Tx, big_tx_sol: f64) {
        let beats_extreme = match self.leg_type {
            SwingType::SwingHigh => tx.price > self.extreme_price,
            SwingType::SwingLow => tx.price < self.extreme_price,
        };
        if beats_extreme {
            self.extreme_at = tx.timestamp_ms;
            self.extreme_price = tx.price;
        }
        if big_tx_sol > 0.0 && tx.sol_amount >= big_tx_sol {
            self.last_big_at = Some(tx.timestamp_ms);
            self.last_big_price = Some(tx.price);
        }
    }

    /// Apply a same-side BUY to a swing-high leg.
    fn apply_buy(&mut self, tx: &Tx, big_tx_sol: f64) {
        self.inflow += tx.sol_amount;
        self.end_at = tx.timestamp_ms;
        self.end_price = tx.price;
        self.trade_count += 1;
        self.consider_pivot(tx, big_tx_sol);
    }

    /// Apply a same-side SELL to a swing-low leg.
    fn apply_sell(&mut self, tx: &Tx, big_tx_sol: f64) {
        self.outflow += tx.sol_amount;
        self.end_at = tx.timestamp_ms;
        self.end_price = tx.price;
        self.trade_count += 1;
        self.consider_pivot(tx, big_tx_sol);
    }

    fn finalize(self) -> SwingLeg {
        // Terminal pivot: last big same-side tx, else the leg's price extreme.
        let (pivot_end_at, pivot_end_price) = match (self.last_big_at, self.last_big_price) {
            (Some(at), Some(price)) => (at, price),
            _ => (self.extreme_at, self.extreme_price),
        };
        SwingLeg {
            leg_type: self.leg_type,
            start_at: self.start_at,
            end_at: self.end_at,
            duration_ms: self.end_at - self.start_at,
            start_price: self.start_price,
            end_price: self.end_price,
            pivot_end_at,
            pivot_end_price,
            inflow: self.inflow,
            outflow: self.outflow,
            net_flow: self.net_flow(),
            trade_count: self.trade_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Combine the absolute (SOL) and relative (percent of the previous leg's net
/// flow) reversal bounds into a single trigger threshold. A value of `0` means
/// "no bound" for that term: it becomes `+inf` so it drops out of the `min` and
/// lets the other term govern. If both terms are `0`, the threshold is `+inf`
/// and no threshold-driven reversal can fire.
fn reversal_threshold(sol: f64, pct: f64, prev_leg_net_flow_abs: f64) -> f64 {
    let sol_bound = if sol > 0.0 { sol } else { f64::INFINITY };
    let pct_bound = if pct > 0.0 {
        pct / 100.0 * prev_leg_net_flow_abs
    } else {
        f64::INFINITY
    };
    sol_bound.min(pct_bound)
}

/// Run swing detection over a token's trades, returning the filtered ledger.
///
/// Generic over any [`TradeRow`] so the lab endpoint (`Trade`), the sweep
/// (`SweepTrade`), and live (`CachedTrade`) all run the identical scan.
pub fn detect_swings<T: TradeRow>(trades: &[T], params: &SwingParams) -> Vec<SwingLeg> {
    let txs = sanitize_and_order(trades);
    let ledger = scan(&txs, params);
    apply_quality_filter(ledger, params)
}

/// Map trades to internal transactions, skip invalid ones, and apply the
/// canonical ordering: `timestamp_ms ASC, slot ASC, position ASC`.
pub(crate) fn sanitize_and_order<T: TradeRow>(trades: &[T]) -> Vec<Tx> {
    let mut ordered: Vec<&T> = trades.iter().filter(|t| t.sol_amount() > 0.0).collect();

    ordered.sort_by(|a, b| {
        a.block_time()
            .timestamp_millis()
            .cmp(&b.block_time().timestamp_millis())
            .then(a.slot().cmp(&b.slot()))
            .then(a.leg_index().cmp(&b.leg_index()))
    });

    let mut prev_post_spot: Option<f64> = None;
    ordered
        .into_iter()
        .map(|t| {
            // Post-trade GMGN spot via the single shared definition (reserve pair
            // → pool → execution). Identical to the chart, live strategies, and
            // the sweep, so a leg detected here is the leg detected live.
            let post_spot = t.chart_spot_price().unwrap_or_else(|| t.execution_price());
            // Spot just before this trade = the previous trade's post-trade spot.
            // The very first trade has no prior state, so fall back to its own
            // post-trade spot.
            let pre_spot = prev_post_spot.unwrap_or(post_spot);
            prev_post_spot = Some(post_spot);
            Tx {
                timestamp_ms: t.block_time().timestamp_millis(),
                side: if t.is_buy() { Side::Buy } else { Side::Sell },
                sol_amount: t.sol_amount(),
                price: post_spot,
                pre_spot,
            }
        })
        .collect()
}

/// Core phase machine. Produces the pre-filter, strictly-alternating ledger.
/// Finalized reversals are pushed during the scan; the active leg and any temp
/// leg still open at end-of-history are appended as well.
pub(crate) fn scan(txs: &[Tx], params: &SwingParams) -> Vec<LegAcc> {
    let mut ledger: Vec<LegAcc> = Vec::new();

    if txs.is_empty() {
        return ledger;
    }

    let mut current_high: Option<LegAcc> = None;
    let mut current_low: Option<LegAcc> = None;
    let mut temp: Option<LegAcc> = None;
    let mut frozen_threshold: f64 = 0.0;

    let big = params.big_tx_sol;
    // A single tx clearing the big-tx threshold confirms a reversal on its own,
    // independent of the accumulated net-flow threshold.
    let is_big = |tx: &Tx| big > 0.0 && tx.sol_amount >= big;

    // Initialization from the first transaction.
    let first = &txs[0];
    let mut phase = match first.side {
        Side::Buy => {
            current_high = Some(LegAcc::seed_high(first, big));
            Phase::SwingHigh
        }
        Side::Sell => {
            current_low = Some(LegAcc::seed_low(first, big));
            Phase::SwingLow
        }
    };

    for tx in &txs[1..] {
        match phase {
            Phase::SwingHigh => match tx.side {
                Side::Buy => current_high.as_mut().unwrap().apply_buy(tx, big),
                Side::Sell => {
                    let snapshot = current_high.as_ref().unwrap().net_flow();
                    frozen_threshold = reversal_threshold(
                        params.high_to_low_threshold_sol,
                        params.high_to_low_threshold_pct,
                        snapshot.abs(),
                    );
                    temp = Some(LegAcc::seed_low(tx, big));
                    phase = Phase::TempSwingLow;

                    if temp.as_ref().unwrap().outflow >= frozen_threshold || is_big(tx) {
                        ledger.push(current_high.take().unwrap());
                        current_low = temp.take();
                        phase = Phase::SwingLow;
                    }
                }
            },

            Phase::TempSwingLow => match tx.side {
                Side::Sell => {
                    let t = temp.as_mut().unwrap();
                    t.apply_sell(tx, big);
                    if t.outflow >= frozen_threshold || is_big(tx) {
                        ledger.push(current_high.take().unwrap());
                        current_low = temp.take();
                        phase = Phase::SwingLow;
                    }
                }
                Side::Buy => {
                    // Sub-threshold: merge temp SELLs back into the swing high.
                    let t = temp.take().unwrap();
                    let h = current_high.as_mut().unwrap();
                    h.outflow += t.outflow;
                    h.trade_count += t.trade_count;
                    phase = Phase::SwingHigh;
                    // The triggering BUY is then counted exactly once here.
                    h.apply_buy(tx, big);
                }
            },

            Phase::SwingLow => match tx.side {
                Side::Sell => current_low.as_mut().unwrap().apply_sell(tx, big),
                Side::Buy => {
                    let snapshot = current_low.as_ref().unwrap().net_flow(); // negative
                    frozen_threshold = reversal_threshold(
                        params.low_to_high_threshold_sol,
                        params.low_to_high_threshold_pct,
                        snapshot.abs(),
                    );
                    temp = Some(LegAcc::seed_high(tx, big));
                    phase = Phase::TempSwingHigh;

                    if temp.as_ref().unwrap().inflow >= frozen_threshold || is_big(tx) {
                        ledger.push(current_low.take().unwrap());
                        current_high = temp.take();
                        phase = Phase::SwingHigh;
                    }
                }
            },

            Phase::TempSwingHigh => match tx.side {
                Side::Buy => {
                    let t = temp.as_mut().unwrap();
                    t.apply_buy(tx, big);
                    if t.inflow >= frozen_threshold || is_big(tx) {
                        ledger.push(current_low.take().unwrap());
                        current_high = temp.take();
                        phase = Phase::SwingHigh;
                    }
                }
                Side::Sell => {
                    // Sub-threshold: merge temp BUYs back into the swing low.
                    let t = temp.take().unwrap();
                    let l = current_low.as_mut().unwrap();
                    l.inflow += t.inflow;
                    l.trade_count += t.trade_count;
                    phase = Phase::SwingLow;
                    // The triggering SELL is then counted exactly once here.
                    l.apply_sell(tx, big);
                }
            },
        }
    }

    match phase {
        Phase::SwingHigh => {
            if let Some(h) = current_high {
                ledger.push(h);
            }
        }
        Phase::TempSwingLow => {
            if let Some(h) = current_high {
                ledger.push(h);
            }
            if let Some(t) = temp {
                ledger.push(t);
            }
        }
        Phase::SwingLow => {
            if let Some(l) = current_low {
                ledger.push(l);
            }
        }
        Phase::TempSwingHigh => {
            if let Some(l) = current_low {
                ledger.push(l);
            }
            if let Some(t) = temp {
                ledger.push(t);
            }
        }
    }

    ledger
}

/// Post-processing pair-based quality filter. Forms fixed, non-overlapping
/// `(swing_high, swing_low)` pairs and discards a pair if one of the legs fail
/// Unpaired legs (leading low / trailing high) are kept as-is.
///
/// NOTE: this pair-drop is **non-causal** (a leg's fate depends on its partner),
/// so the swing1 phase classifier deliberately does NOT use it — it walks the
/// raw [`scan`] ledger with causal per-leg gates only. This filter is retained
/// for the lab swing-chart endpoint, which is a cold batch path.
fn apply_quality_filter(ledger: Vec<LegAcc>, params: &SwingParams) -> Vec<SwingLeg> {
    let fails = |leg: &LegAcc| -> bool {
        let duration_ms = leg.end_at - leg.start_at;

        // Per-leg-type delta % and net-flow-per-second bounds (0 = no bound).
        let (min_delta_pct, max_delta_pct, min_nf_per_sec, max_nf_per_sec) = match leg.leg_type {
            SwingType::SwingHigh => (
                params.swing_high_min_delta_pct,
                params.swing_high_max_delta_pct,
                params.swing_high_min_net_flow_per_sec,
                params.swing_high_max_net_flow_per_sec,
            ),
            SwingType::SwingLow => (
                params.swing_low_min_delta_pct,
                params.swing_low_max_delta_pct,
                params.swing_low_min_net_flow_per_sec,
                params.swing_low_max_net_flow_per_sec,
            ),
        };
        // Compared by magnitude so swing-low legs (negative delta/net flow) use
        // the same positive thresholds as swing highs.
        let delta_pct_abs = if leg.start_price != 0.0 {
            ((leg.end_price - leg.start_price) / leg.start_price * 100.0).abs()
        } else {
            0.0
        };
        // Rate is undefined for instantaneous (0-duration) legs, so the ratio
        // bounds are simply skipped for them rather than dividing by zero.
        let net_flow_per_sec_abs = if duration_ms > 0 {
            Some((leg.net_flow() / (duration_ms as f64 / 1000.0)).abs())
        } else {
            None
        };

        leg.trade_count < params.min_leg_trades
            || duration_ms < params.min_leg_duration_ms
            || leg.net_flow().abs() < params.min_leg_net_flow
            || (leg.inflow + leg.outflow) < params.min_leg_volume
            || (params.max_leg_trades > 0 && leg.trade_count > params.max_leg_trades)
            || (params.max_leg_duration_ms > 0 && duration_ms > params.max_leg_duration_ms)
            || (params.max_leg_net_flow > 0.0 && leg.net_flow().abs() > params.max_leg_net_flow)
            || (params.max_leg_volume > 0.0 && (leg.inflow + leg.outflow) > params.max_leg_volume)
            || (min_delta_pct > 0.0 && delta_pct_abs < min_delta_pct)
            || (max_delta_pct > 0.0 && delta_pct_abs > max_delta_pct)
            || (min_nf_per_sec > 0.0 && net_flow_per_sec_abs.is_some_and(|r| r < min_nf_per_sec))
            || (max_nf_per_sec > 0.0 && net_flow_per_sec_abs.is_some_and(|r| r > max_nf_per_sec))
    };

    let mut out: Vec<SwingLeg> = Vec::with_capacity(ledger.len());
    let mut i = 0;
    while i < ledger.len() {
        let is_pair = i + 1 < ledger.len()
            && ledger[i].leg_type == SwingType::SwingHigh
            && ledger[i + 1].leg_type == SwingType::SwingLow;

        if is_pair {
            if !(fails(&ledger[i]) || fails(&ledger[i + 1])) {
                out.push(ledger[i].clone().finalize());
                out.push(ledger[i + 1].clone().finalize());
            }
            i += 2;
        } else {
            // Unpaired leg: ignored by the filter, kept as-is.
            out.push(ledger[i].clone().finalize());
            i += 1;
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Chain-of-swings stats
// ---------------------------------------------------------------------------

// `ChainStats` lives in `trading_core::analyzers` (shared with the core token-list
// sort); re-exported so `swing_analyzer::ChainStats` paths resolve.
pub use crate::analyzers::ChainStats;

/// Reduce a token's alternating legs to high→low pairs and group them into
/// chains, mirroring the frontend exactly: legs are walked in `start_at` order; a
/// *pair* is a `SwingHigh` immediately followed by a `SwingLow` (unpaired legs are
/// skipped); two consecutive pairs *link* when the idle gap
/// (`next.start_at − current.end_at`) is within `chain_latency_ms`; a *chain* is a
/// maximal run of ≥ 2 linked pairs.
pub fn compute_chain_stats(swings: &[SwingLeg], chain_latency_ms: i64) -> ChainStats {
    // (high start_at, low end_at) per pair, in time order.
    let mut sorted: Vec<&SwingLeg> = swings.iter().collect();
    sorted.sort_by_key(|s| s.start_at);
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let high = sorted[i];
        if high.leg_type == SwingType::SwingHigh
            && matches!(sorted.get(i + 1), Some(low) if low.leg_type == SwingType::SwingLow)
        {
            pairs.push((high.start_at, sorted[i + 1].end_at));
            i += 2; // consume the pair
        } else {
            i += 1; // unpaired leg — skip
        }
    }

    let mut max_run: u32 = 0;
    let mut cur_run: u32 = 0; // 0 = no chain currently open
    let mut chain_count: u32 = 0;
    for k in 0..pairs.len().saturating_sub(1) {
        let gap = pairs[k + 1].0 - pairs[k].1;
        if gap <= chain_latency_ms {
            if cur_run == 0 {
                cur_run = 2; // this link joins pairs k and k+1 — a new chain opens
                chain_count += 1;
            } else {
                cur_run += 1; // extend the chain by one more pair
            }
            if cur_run > max_run {
                max_run = cur_run;
            }
        } else {
            cur_run = 0; // gap too large — chain breaks here
        }
    }

    ChainStats {
        swing_pairs: pairs.len() as u32,
        max_seq_pairs: max_run,
        chain_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;

    fn buy(ms: i64, sol: f64, tokens: f64, _vsol_post: f64, _vtok_post: f64, leg: u32) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            sol,
            tokens as u64,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        t.leg_index = leg;
        t
    }

    fn sell(ms: i64, sol: f64, tokens: f64, _vsol_post: f64, _vtok_post: f64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Sell,
            sol,
            tokens as u64,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        t
    }

    #[test]
    fn swing_high_first_trade_start_falls_back_to_execution_price() {
        // The very first trade has no prior curve state, so `start_price` falls
        // back to its own execution price.
        let trades = vec![buy(1_000, 10.0, 1_000_000.0, 110.0, 900_000.0, 0)];
        let legs = detect_swings(&trades, &SwingParams::default());
        assert_eq!(legs.len(), 1);
        assert!((legs[0].start_price - trades[0].execution_price()).abs() < 1e-15);
        assert!((legs[0].end_price - trades[0].price_per_token).abs() < 1e-15);
    }

    #[test]
    fn confirmed_swing_high_start_uses_pre_trade_spot() {
        let params = SwingParams {
            high_to_low_threshold_sol: 1.0,
            high_to_low_threshold_pct: 100.0,
            low_to_high_threshold_sol: 1.0,
            low_to_high_threshold_pct: 100.0,
            min_leg_trades: 1,
            ..Default::default()
        };
        // Distinct execution prices so pre-trade spot (= prior trade's post spot)
        // differs from the opening buy's own execution price.
        let trades = vec![
            buy(1_000, 5.0, 500_000.0, 105.0, 950_000.0, 0),
            sell(2_000, 2.0, 100_000.0, 103.0, 1_150_000.0),
            buy(3_000, 3.0, 300_000.0, 106.0, 850_000.0, 0),
        ];
        let legs = detect_swings(&trades, &params);
        let high = legs
            .iter()
            .filter(|l| l.leg_type == SwingType::SwingHigh)
            .find(|l| l.start_at == 3_000)
            .unwrap();
        let opener = &trades[2];
        let prev = &trades[1];
        // start_at is the opening buy's timestamp; start_price is the spot just
        // before it (the previous trade's post-trade spot), not its execution.
        assert!((high.start_price - prev.price_per_token).abs() < 1e-15);
        assert!((high.start_price - opener.execution_price()).abs() > 1e-15);
        assert!((high.end_price - opener.price_per_token).abs() < 1e-15);
    }

    fn leg(leg_type: SwingType, start_at: i64, end_at: i64) -> SwingLeg {
        SwingLeg {
            leg_type,
            start_at,
            end_at,
            duration_ms: end_at - start_at,
            start_price: 0.0,
            end_price: 0.0,
            pivot_end_at: end_at,
            pivot_end_price: 0.0,
            inflow: 0.0,
            outflow: 0.0,
            net_flow: 0.0,
            trade_count: 1,
        }
    }

    #[test]
    fn chain_stats_link_pairs_within_latency() {
        use SwingType::{SwingHigh, SwingLow};
        // Three high→low pairs. Pair0 ends @200, pair1 starts @300 (gap 100);
        // pair1 ends @500, pair2 starts @560 (gap 60). With latency 100 all three
        // link into ONE chain of 3 pairs.
        let swings = vec![
            leg(SwingHigh, 100, 150),
            leg(SwingLow, 160, 200),
            leg(SwingHigh, 300, 350),
            leg(SwingLow, 360, 500),
            leg(SwingHigh, 560, 600),
            leg(SwingLow, 610, 700),
        ];
        let s = compute_chain_stats(&swings, 100);
        assert_eq!(s.swing_pairs, 3);
        assert_eq!(s.max_seq_pairs, 3);
        assert_eq!(s.chain_count, 1);

        // Tighten the latency so only the second gap (60) links → one chain of 2,
        // the first pair left isolated.
        let s = compute_chain_stats(&swings, 60);
        assert_eq!(s.swing_pairs, 3);
        assert_eq!(s.max_seq_pairs, 2);
        assert_eq!(s.chain_count, 1);

        // No gap links → no chains.
        let s = compute_chain_stats(&swings, 0);
        assert_eq!(s.swing_pairs, 3);
        assert_eq!(s.max_seq_pairs, 0);
        assert_eq!(s.chain_count, 0);
    }

    #[test]
    fn chain_stats_skip_unpaired_legs() {
        use SwingType::{SwingHigh, SwingLow};
        // Leading lone low + trailing lone high are not part of any pair.
        let swings = vec![
            leg(SwingLow, 50, 80),
            leg(SwingHigh, 100, 150),
            leg(SwingLow, 160, 200),
            leg(SwingHigh, 900, 950),
        ];
        let s = compute_chain_stats(&swings, 1_000);
        assert_eq!(s.swing_pairs, 1);
        assert_eq!(s.max_seq_pairs, 0);
        assert_eq!(s.chain_count, 0);
    }
}
