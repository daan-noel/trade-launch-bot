//! Wire DTOs for the family-search job. The report is [`super::report::Report`].
//!
//! **Every knob a run reads is here** — that is charter D5's third line made
//! structural. Rule search's handler overrides `buy_amount_sol`,
//! `max_concurrent_tokens`, and `max_total_tokens` from the incumbent; because cost is
//! U-shaped under `pumpfun_impact` and the expectancy floor derives from buy size, an
//! incumbent silently moves both the economics and the Refuse/Candidate bar, and the
//! caps additionally change which tokens are entered at all. Family search takes all
//! three from the request only. [`StartFamilySearchBody::incumbent_rule_id`] exists
//! solely to render a **display column** and touches nothing else (D6).

use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use trading_core::strategies::kernel::CostModelKind;
use trading_core::strategies::paper_fill::FillModel;

use crate::family_search::family::Axis;
use crate::family_search::gates::DEFAULT_FRESHNESS_SLACK_SECS;
use crate::family_search::generator::DEFAULT_SLOTS;
use crate::sweep::registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL;

/// The varied axis, on the wire. Absent ⇒ resolve it from the table (the axis with
/// the most siblings).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisName {
    CuLimit,
    CuPrice,
    InitBuy,
    MaxCost,
    SpendableIn,
    FirstSlotBuy,
    FirstSlotSell,
}

impl From<AxisName> for Axis {
    fn from(a: AxisName) -> Self {
        match a {
            AxisName::CuLimit => Axis::CuLimit,
            AxisName::CuPrice => Axis::CuPrice,
            AxisName::InitBuy => Axis::InitBuyLamports,
            AxisName::MaxCost => Axis::MaxCostLamports,
            AxisName::SpendableIn => Axis::SpendableLamportsIn,
            AxisName::FirstSlotBuy => Axis::FirstSlotBuyLamports,
            AxisName::FirstSlotSell => Axis::FirstSlotSellLamports,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StartFamilySearchBody {
    /// The **target**: the cohort the reported level comes from. Its siblings are
    /// resolved mechanically off the `fingerprints` table.
    pub fingerprint_id: Uuid,
    #[serde(default, alias = "since")]
    pub created_after: Option<DateTime<Utc>>,
    #[serde(default, alias = "until")]
    pub created_before: Option<DateTime<Utc>>,
    /// Physics or operator choice — never read off a rule.
    #[serde(default = "default_buy")]
    pub buy_amount_sol: f64,
    #[serde(default)]
    pub fill_model: FillModel,
    #[serde(default)]
    pub cost_model: CostModelKind,
    /// Absent ⇒ ON (this job's default).
    #[serde(default = "default_copycat_on")]
    pub skip_duplicate_identity: bool,
    #[serde(default)]
    pub max_concurrent_tokens: u32,
    #[serde(default)]
    pub max_total_tokens: u32,
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
    /// Pin the family's varied axis. Absent ⇒ resolve it.
    #[serde(default)]
    pub varied_axis: Option<AxisName>,
    /// Candidate slots per cohort.
    #[serde(default = "default_slots")]
    pub slots: usize,
    /// How far the requested `until` may outrun the lake's newest print before the
    /// run is refused (D7).
    #[serde(default = "default_freshness_slack")]
    pub freshness_slack_secs: i64,
    /// How much headroom over one round trip the cohort's typical best exit must
    /// leave, in bands, before a search runs (D8). `0.0` (the default) refuses only
    /// the unarguable case — the best available exit does not clear its own costs.
    /// `1.0` is the stricter bar the dump-scalp result sets.
    #[serde(default = "default_cost_clearance_margin")]
    pub cost_clearance_margin: f64,
    /// **Standing exit terms** (D10): mechanical alarms the operator always wants,
    /// written as the board prints them — `"liquidity >= 85"` (sell at migration),
    /// `"untagged_buy(2s) >= 0.9"`. Each rides into every candidate **and** the ungated
    /// control, so the numbers describe a rule someone would actually run; none is
    /// searched, ablated, or credited with the rule's edge. A term that does not parse
    /// fails the run rather than being dropped.
    #[serde(default)]
    pub standing_exit: Vec<String>,
    /// Absolute win-rate floor, in percent, on top of the cohort's own bar (D11). The
    /// draft must clear **both** this and the ungated control's win rate — an entry
    /// gate that does not enter more safely than buying everything is not filtering.
    /// `0.0` (the default) leaves the control as the only bar.
    #[serde(default)]
    pub min_win_rate_pct: f64,
    /// Closes a candidate needs before its win rate is believed. Three wins in four
    /// trades is not a 75% win rate.
    #[serde(default = "default_min_closed")]
    pub min_closed: u64,
    /// **Display only** (D6): an incumbent is an artifact, not a baseline. It is
    /// scored as one more column and supplies no buy size, no cap, no threshold, and
    /// no structure. Off by default.
    #[serde(default)]
    pub incumbent_rule_id: Option<Uuid>,
}

/// Closes below which a win rate is not yet a measurement.
pub const DEFAULT_MIN_CLOSED: u64 = 8;

fn default_buy() -> f64 {
    SWEEP_DEFAULT_BUY_AMOUNT_SOL
}
fn default_copycat_on() -> bool {
    true
}
fn default_token_cap() -> usize {
    crate::sweep::registry::DEFAULT_TOKEN_CAP
}
fn default_slots() -> usize {
    DEFAULT_SLOTS
}
fn default_freshness_slack() -> i64 {
    DEFAULT_FRESHNESS_SLACK_SECS
}
fn default_cost_clearance_margin() -> f64 {
    crate::family_search::gates::DEFAULT_COST_CLEARANCE_MARGIN
}
fn default_min_closed() -> u64 {
    DEFAULT_MIN_CLOSED
}
