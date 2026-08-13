//! Wire DTOs for the rule-search job. The report is [`super::report::Report`].

use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use trading_core::strategies::kernel::CostModelKind;
use trading_core::strategies::paper_fill::FillModel;

use crate::sweep::registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL;

#[derive(Debug, Deserialize)]
pub struct StartRuleSearchBody {
    pub fingerprint_id: Uuid,
    #[serde(default, alias = "since")]
    pub created_after: Option<DateTime<Utc>>,
    #[serde(default, alias = "until")]
    pub created_before: Option<DateTime<Utc>>,
    #[serde(default = "default_buy")]
    pub buy_amount_sol: f64,
    #[serde(default)]
    pub fill_model: FillModel,
    #[serde(default = "default_cost")]
    pub cost_model: CostModelKind,
    /// Absent ⇒ ON (this job's default). Simulate inherits app_settings when absent;
    /// the form sends an explicit bool.
    #[serde(default = "default_copycat_on")]
    pub skip_duplicate_identity: bool,
    #[serde(default)]
    pub incumbent_rule_id: Option<Uuid>,
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
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
