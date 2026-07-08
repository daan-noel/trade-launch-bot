//! Request + plan DTOs for post-launch management (token-management-plan.md).
//!
//! Three primitives (Sell/Buy/Consolidate) over a wallet selection + a sizing
//! mode produce an [`ActionPlan`] — a list of per-wallet legs — which the operator
//! previews before it executes. Phase 2 wires the Sell primitive with
//! `pct_of_holdings` sizing; the other kinds/sizings are typed here so the wire
//! contract is stable across phases.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which wallets an action targets. `wallet_ids` (when non-empty) wins over
/// `role`; with both unset the action targets every holder of the mint.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WalletSelection {
    /// Restrict to a wallet role (`dev` | `bundler` | …). `None` = any role.
    #[serde(default)]
    pub role: Option<String>,
    /// Explicit wallet ids — overrides `role` when non-empty.
    #[serde(default)]
    pub wallet_ids: Vec<Uuid>,
}

impl WalletSelection {
    /// Does this position's (wallet_id, role) match the selection?
    pub fn matches(&self, wallet_id: Uuid, role: &str) -> bool {
        if !self.wallet_ids.is_empty() {
            return self.wallet_ids.contains(&wallet_id);
        }
        match &self.role {
            Some(r) => r == role,
            None => true,
        }
    }
}

/// The operator's action request (from the Manage panel). `size` is interpreted
/// per `sizing`: for `pct_of_holdings` it's a percent 0–100.
#[derive(Debug, Clone, Deserialize)]
pub struct ManageRequest {
    /// `sell` (Phase 2). `buy` / `consolidate` land in Phase 3.
    pub kind: String,
    /// `pct_of_holdings` (Phase 2).
    pub sizing: String,
    /// Sizing magnitude (percent for `pct_of_holdings`).
    pub size: f64,
    #[serde(default)]
    pub selection: WalletSelection,
}

/// One planned per-wallet leg. Amount fields are exact base-unit integers
/// (`_base` token / `_quote` quote). The execution-outcome fields are `None` on a
/// preview and filled in by the executor for the audit row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLeg {
    pub wallet_id: Uuid,
    pub role: String,
    /// Canonical token account the sell routes through (from the position row).
    pub token_account: Option<String>,
    /// `sell` | `buy`.
    pub side: String,
    /// Tokens to sell (sell side), token base units.
    pub amount_base: i64,
    /// Estimated proceeds at the current spot price (quote base units) — preview
    /// display only; realized proceeds come from the feed after execution.
    pub est_quote: i64,
    /// `confirmed` | `failed`, set by the executor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Confirmed sell tx signature (base58), set by the executor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A previewable action plan — the legs plus roll-up totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    pub mint_address: String,
    pub kind: String,
    pub sizing: String,
    pub legs: Vec<PlanLeg>,
    pub total_amount_base: i64,
    pub total_est_quote: i64,
}

impl ActionPlan {
    /// Build from computed legs, summing the roll-ups (SSOT — totals never drift
    /// from the legs).
    pub fn from_legs(mint_address: &str, kind: &str, sizing: &str, legs: Vec<PlanLeg>) -> Self {
        let total_amount_base = legs.iter().map(|l| l.amount_base).sum();
        let total_est_quote = legs.iter().map(|l| l.est_quote).sum();
        Self {
            mint_address: mint_address.to_string(),
            kind: kind.to_string(),
            sizing: sizing.to_string(),
            legs,
            total_amount_base,
            total_est_quote,
        }
    }
}
