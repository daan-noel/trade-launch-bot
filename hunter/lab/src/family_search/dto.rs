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
            AxisName::InitBuy => Axis::InitBuy,
            AxisName::MaxCost => Axis::MaxCost,
            AxisName::SpendableIn => Axis::SpendableIn,
            AxisName::FirstSlotBuy => Axis::FirstSlotBuy,
            AxisName::FirstSlotSell => Axis::FirstSlotSell,
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
    #[serde(default = "default_cost")]
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
    /// **Display only** (D6): an incumbent is an artifact, not a baseline. It is
    /// scored as one more column and supplies no buy size, no cap, no threshold, and
    /// no structure. Off by default.
    #[serde(default)]
    pub incumbent_rule_id: Option<Uuid>,
}

fn default_buy() -> f64 {
    SWEEP_DEFAULT_BUY_AMOUNT_SOL
}
fn default_cost() -> CostModelKind {
    CostModelKind::PumpfunImpact
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
