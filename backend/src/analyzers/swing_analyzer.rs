//! Token Swing Analyzer.
//!
//! Detects alternating buy-dominant (swing high) and sell-dominant (swing low)
//! legs in a token's trade history. See
//! `@project_plans/token_swing_analyzer_spec_using_TA_terms.md` for the full
//! specification this implementation follows.

use serde::{Deserialize, Serialize};

use crate::models::trade::{Trade, TradeType};

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// Tunable parameters for swing detection. All fields are optional in the JSON
/// body and fall back to the defaults below.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwingParams {
    // Reversal thresholds: Swing High -> Swing Low
    #[serde(default = "default_high_to_low_sol")]
    pub high_to_low_threshold_sol: f64,
    #[serde(default = "default_pct")]
    pub high_to_low_threshold_pct: f64, // 0-100 (real percent)

    // Reversal thresholds: Swing Low -> Swing High
    #[serde(default = "default_low_to_high_sol")]
    pub low_to_high_threshold_sol: f64,
    #[serde(default = "default_pct")]
    pub low_to_high_threshold_pct: f64, // 0-100 (real percent)

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

struct Tx {
    timestamp_ms: i64,
    slot: u64,
    position: u32,
    side: Side,
    sol_amount: f64,
    /// Post-trade spot (`price_per_token`).
    price: f64,
    /// Pre-trade spot (for leg `start_price`).
    price_before: f64,
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
struct LegAcc {
    leg_type: SwingType,
    start_at: i64,
    end_at: i64,
    start_price: f64,
    end_price: f64,
    inflow: f64,
    outflow: f64,
    trade_count: u32,
}

impl LegAcc {
    /// Create a swing-high leg seeded from a BUY (fully consumes the tx).
    fn seed_high(tx: &Tx) -> Self {
        Self {
            leg_type: SwingType::SwingHigh,
            start_at: tx.timestamp_ms,
            end_at: tx.timestamp_ms,
            start_price: tx.price_before,
            end_price: tx.price,
            inflow: tx.sol_amount,
            outflow: 0.0,
            trade_count: 1,
        }
    }

    /// Create a swing-low leg seeded from a SELL (fully consumes the tx).
    fn seed_low(tx: &Tx) -> Self {
        Self {
            leg_type: SwingType::SwingLow,
            start_at: tx.timestamp_ms,
            end_at: tx.timestamp_ms,
            start_price: tx.price_before,
            end_price: tx.price,
            inflow: 0.0,
            outflow: tx.sol_amount,
            trade_count: 1,
        }
    }

    fn net_flow(&self) -> f64 {
        self.inflow - self.outflow
    }

    /// Apply a same-side BUY to a swing-high leg.
    fn apply_buy(&mut self, tx: &Tx) {
        self.inflow += tx.sol_amount;
        self.end_at = tx.timestamp_ms;
        self.end_price = tx.price;
        self.trade_count += 1;
    }

    /// Apply a same-side SELL to a swing-low leg.
    fn apply_sell(&mut self, tx: &Tx) {
        self.outflow += tx.sol_amount;
        self.end_at = tx.timestamp_ms;
        self.end_price = tx.price;
        self.trade_count += 1;
    }

    fn finalize(self) -> SwingLeg {
        SwingLeg {
            leg_type: self.leg_type,
            start_at: self.start_at,
            end_at: self.end_at,
            duration_ms: self.end_at - self.start_at,
            start_price: self.start_price,
            end_price: self.end_price,
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

/// Run swing detection over a token's trades, returning the filtered ledger.
pub fn detect_swings(trades: &[Trade], params: &SwingParams) -> Vec<SwingLeg> {
    let txs = sanitize_and_order(trades);
    let ledger = scan(&txs, params);
    apply_quality_filter(ledger, params)
}

/// Map trades to internal transactions, skip invalid ones, and apply the
/// canonical ordering: `timestamp_ms ASC, slot ASC, position ASC`.
fn sanitize_and_order(trades: &[Trade]) -> Vec<Tx> {
    let mut ordered: Vec<&Trade> = trades.iter().filter(|t| t.sol_amount > 0.0).collect();

    ordered.sort_by(|a, b| {
        a.block_time
            .timestamp_millis()
            .cmp(&b.block_time.timestamp_millis())
            .then(a.slot.cmp(&b.slot))
            .then(a.leg_index.cmp(&b.leg_index))
    });

    let mut txs: Vec<Tx> = Vec::with_capacity(ordered.len());
    for (i, t) in ordered.iter().enumerate() {
        let prev_post = i.checked_sub(1).map(|j| txs[j].price);
        txs.push(Tx {
            timestamp_ms: t.block_time.timestamp_millis(),
            slot: t.slot,
            position: t.leg_index,
            side: match t.trade_type {
                TradeType::Buy => Side::Buy,
                TradeType::Sell => Side::Sell,
            },
            sol_amount: t.sol_amount,
            price: t.price_per_token,
            price_before: t.price_before_execution(prev_post),
        });
    }

    txs
}

/// Core phase machine. Produces the pre-filter, strictly-alternating ledger.
/// Finalized reversals are pushed during the scan; the active leg and any temp
/// leg still open at end-of-history are appended as well.
fn scan(txs: &[Tx], params: &SwingParams) -> Vec<LegAcc> {
    let mut ledger: Vec<LegAcc> = Vec::new();

    if txs.is_empty() {
        return ledger;
    }

    let mut current_high: Option<LegAcc> = None;
    let mut current_low: Option<LegAcc> = None;
    let mut temp: Option<LegAcc> = None;
    let mut frozen_threshold: f64 = 0.0;

    // Initialization from the first transaction.
    let first = &txs[0];
    let mut phase = match first.side {
        Side::Buy => {
            current_high = Some(LegAcc::seed_high(first));
            Phase::SwingHigh
        }
        Side::Sell => {
            current_low = Some(LegAcc::seed_low(first));
            Phase::SwingLow
        }
    };

    for tx in &txs[1..] {
        match phase {
            Phase::SwingHigh => match tx.side {
                Side::Buy => current_high.as_mut().unwrap().apply_buy(tx),
                Side::Sell => {
                    let snapshot = current_high.as_ref().unwrap().net_flow();
                    frozen_threshold = params
                        .high_to_low_threshold_sol
                        .min(params.high_to_low_threshold_pct / 100.0 * snapshot.abs());
                    temp = Some(LegAcc::seed_low(tx));
                    phase = Phase::TempSwingLow;

                    if temp.as_ref().unwrap().outflow >= frozen_threshold {
                        ledger.push(current_high.take().unwrap());
                        current_low = temp.take();
                        phase = Phase::SwingLow;
                    }
                }
            },

            Phase::TempSwingLow => match tx.side {
                Side::Sell => {
                    let t = temp.as_mut().unwrap();
                    t.apply_sell(tx);
                    if t.outflow >= frozen_threshold {
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
                    h.apply_buy(tx);
                }
            },

            Phase::SwingLow => match tx.side {
                Side::Sell => current_low.as_mut().unwrap().apply_sell(tx),
                Side::Buy => {
                    let snapshot = current_low.as_ref().unwrap().net_flow(); // negative
                    frozen_threshold = params
                        .low_to_high_threshold_sol
                        // .max(params.low_to_high_threshold_pct / 100.0 * snapshot.abs());
                        .min(params.low_to_high_threshold_sol);
                    temp = Some(LegAcc::seed_high(tx));
                    phase = Phase::TempSwingHigh;

                    if temp.as_ref().unwrap().inflow >= frozen_threshold {
                        ledger.push(current_low.take().unwrap());
                        current_high = temp.take();
                        phase = Phase::SwingHigh;
                    }
                }
            },

            Phase::TempSwingHigh => match tx.side {
                Side::Buy => {
                    let t = temp.as_mut().unwrap();
                    t.apply_buy(tx);
                    if t.inflow >= frozen_threshold {
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
                    l.apply_sell(tx);
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
/// `(swing_high, swing_low)` pairs and discards a pair only when BOTH legs
/// fail. Unpaired legs (leading low / trailing high) are kept as-is.
fn apply_quality_filter(ledger: Vec<LegAcc>, params: &SwingParams) -> Vec<SwingLeg> {
    let fails = |leg: &LegAcc| -> bool {
        let duration_ms = leg.end_at - leg.start_at;
        leg.trade_count < params.min_leg_trades
            || duration_ms < params.min_leg_duration_ms
            || leg.net_flow().abs() < params.min_leg_net_flow
            || (leg.inflow + leg.outflow) < params.min_leg_volume
            || (params.max_leg_trades > 0 && leg.trade_count > params.max_leg_trades)
            || (params.max_leg_duration_ms > 0 && duration_ms > params.max_leg_duration_ms)
            || (params.max_leg_net_flow > 0.0 && leg.net_flow().abs() > params.max_leg_net_flow)
            || (params.max_leg_volume > 0.0 && (leg.inflow + leg.outflow) > params.max_leg_volume)
    };

    let mut out: Vec<SwingLeg> = Vec::with_capacity(ledger.len());
    let mut i = 0;
    while i < ledger.len() {
        let is_pair = i + 1 < ledger.len()
            && ledger[i].leg_type == SwingType::SwingHigh
            && ledger[i + 1].leg_type == SwingType::SwingLow;

        if is_pair {
            if !(fails(&ledger[i]) && fails(&ledger[i + 1])) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn buy(
        ms: i64,
        sol: f64,
        tokens: f64,
        vsol_post: f64,
        vtok_post: f64,
        leg: u32,
    ) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Buy,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        t.leg_index = leg;
        t.virtual_sol_reserves = Some(vsol_post);
        t.virtual_token_reserves = Some(vtok_post);
        t.apply_curve_price();
        t
    }

    fn sell(ms: i64, sol: f64, tokens: f64, vsol_post: f64, vtok_post: f64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            TradeType::Sell,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        t.virtual_sol_reserves = Some(vsol_post);
        t.virtual_token_reserves = Some(vtok_post);
        t.apply_curve_price();
        t
    }

    #[test]
    fn swing_high_start_uses_pre_trade_price() {
        let trades = vec![buy(1_000, 10.0, 1_000_000.0, 110.0, 900_000.0, 0)];
        let legs = detect_swings(&trades, &SwingParams::default());
        assert_eq!(legs.len(), 1);
        let pre = trades[0].price_before_from_reserves().unwrap();
        assert!((legs[0].start_price - pre).abs() < 1e-15);
        assert!(legs[0].start_price < legs[0].end_price);
    }

    #[test]
    fn confirmed_swing_high_after_reversal_uses_pre_first_buy() {
        let params = SwingParams {
            high_to_low_threshold_sol: 1.0,
            high_to_low_threshold_pct: 100.0,
            low_to_high_threshold_sol: 1.0,
            low_to_high_threshold_pct: 100.0,
            min_leg_trades: 1,
            ..Default::default()
        };
        let trades = vec![
            buy(1_000, 5.0, 500_000.0, 105.0, 950_000.0, 0),
            sell(2_000, 2.0, 200_000.0, 103.0, 1_150_000.0),
            buy(3_000, 3.0, 300_000.0, 106.0, 850_000.0, 0),
        ];
        let legs = detect_swings(&trades, &params);
        let high = legs
            .iter()
            .filter(|l| l.leg_type == SwingType::SwingHigh)
            .find(|l| l.start_at == 3_000)
            .unwrap();
        let opener = &trades[2];
        let pre = opener.price_before_from_reserves().unwrap();
        assert!((high.start_price - pre).abs() < 1e-15);
        assert!((high.end_price - opener.price_per_token).abs() < 1e-15);
    }
}
