use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a position held in a token.
/// A position is created when a buy rule is triggered, and closed when an exit rule is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: Uuid,
    /// Token mint address (SPL token address).
    pub mint: String,
    /// Wallet address that owns this position.
    pub wallet: String,
    /// Token program id used for this position (SPL legacy or Token-2022).
    pub token_program_id: Option<String>,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual `entry_*` fill. Set later via
    /// [`Position::set_target`], not at construction; `None` until armed (and for
    /// legacy rows / paths that never arm, e.g. backtest). `target_price` is the
    /// trigger trade's price, `target_token_amount` its **token** count,
    /// `target_time` its block time, `target_tx` its signature. SOL is never
    /// stored — derived at display as `price × tokens`. The gap vs. `entry_*` is
    /// derived later, not stored.
    pub target_price: Option<f64>,
    pub target_token_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    /// Entry price (SOL per token) when the position was opened.
    pub entry_price: Option<f64>,
    /// Amount of tokens bought at entry.
    pub entry_token_amount: Option<f64>,
    /// On-chain block time of the confirmed buy trade.
    pub entry_time: Option<DateTime<Utc>>,
    /// Transaction signature(s) that made up the entry fill. Single-leg today (one
    /// snipe buy), but a JSONB array so a scaled-in, multi-leg entry needs no
    /// schema change. Empty until the on-chain fill is adopted (per-signature
    /// attribution — each concurrent same-token position tracks its OWN buy tx, so
    /// the shared `(wallet, mint)` feed can never adopt another position's fill).
    pub entry_tx_signatures: Vec<String>,
    /// Exit price (SOL per token) when the position was closed.
    pub exit_price: Option<f64>,
    /// Amount of tokens sold at exit.
    pub exit_token_amount: Option<f64>,
    /// On-chain block time of the confirmed sell trade.
    pub exit_time: Option<DateTime<Utc>>,
    /// Transaction signature(s) that made up the exit fill — genuinely multi-leg
    /// (the sell-confirm loop retries / re-routes across migration, each leg its
    /// own tx). The exit is confirmed by summing *these* signatures' token legs
    /// against `entry_token_amount`, so concurrent positions never confirm against
    /// each other's sells. Empty until at least one sell lands.
    pub exit_tx_signatures: Vec<String>,
    /// "Holding" — owns tokens, exit not yet triggered | "ExitPending" — exit
    /// triggered, sell/confirmation in flight | "End" — exited cleanly |
    /// "ExitFailed" — terminal: the exit attempt ran and failed.
    pub status: PositionStatus,
    /// Strategy name (e.g., "TPSL1" or "TPSL2").
    pub strategy: String,
    /// Rule ID from the strategy rules table that triggered this position.
    pub rule_id: Uuid,
    /// Why the position exited — one of the exit-ladder reasons ("TakeProfit",
    /// "StopLoss", "TrailingStop", "Stall", "TimeStop", "LiquidityExit"). `None`
    /// while still Holding/ExitPending (or for legacy rows predating this field).
    pub exit_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PositionStatus {
    Holding,
    /// Created before the buy fill lands. Transitions to `Holding` via
    /// `update_entry` once the on-chain fill is adopted. Not counted toward
    /// the per-rule cap and excluded from frontend summary tallies.
    PendingEntry,
    ExitPending,
    End,
    /// Terminal: the exit attempt completed and failed (real: sell retries
    /// exhausted without clearing the balance; paper: no confirming trade
    /// indexed within the poll window). The position is never re-evaluated.
    ExitFailed,
}

impl std::fmt::Display for PositionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Holding => write!(f, "Holding"),
            Self::PendingEntry => write!(f, "PendingEntry"),
            Self::ExitPending => write!(f, "ExitPending"),
            Self::End => write!(f, "End"),
            Self::ExitFailed => write!(f, "ExitFailed"),
        }
    }
}

impl std::str::FromStr for PositionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Holding" => Ok(Self::Holding),
            "PendingEntry" => Ok(Self::PendingEntry),
            "ExitPending" => Ok(Self::ExitPending),
            "End" => Ok(Self::End),
            "ExitFailed" => Ok(Self::ExitFailed),
            _ => Err(format!("Unknown status: {}", s)),
        }
    }
}

impl Position {
    pub fn new(
        mint: String,
        wallet: String,
        strategy: String,
        rule_id: Uuid,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            mint,
            wallet,
            token_program_id: None,
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: None,
            entry_token_amount: None,
            entry_time: None,
            entry_tx_signatures: Vec::new(),
            exit_price: None,
            exit_token_amount: None,
            exit_time: None,
            exit_tx_signatures: Vec::new(),
            status: PositionStatus::PendingEntry,
            strategy,
            rule_id,
            exit_reason: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Record the target (trigger-trade) snapshot — the scalp-entry signal trade
    /// that armed this position. Set before the entry fill lands; `entry_*` is
    /// filled independently later, so the two can be compared to derive the gap.
    ///
    /// In-memory mutator parallel to the repo's `update_target` (the live arming
    /// path persists via that, syncing the RETURNed row); kept as the model-level
    /// setter for callers that mutate a `Position` before a bulk write.
    #[allow(dead_code)]
    pub fn set_target(
        &mut self,
        price: f64,
        amount: f64,
        time: DateTime<Utc>,
        tx: String,
    ) {
        self.target_price = Some(price);
        self.target_token_amount = Some(amount);
        self.target_time = Some(time);
        self.target_tx = Some(tx);
        self.updated_at = Utc::now();
    }

    /// Mark the position as pending exit while the sell is executing.
    pub fn mark_exit_pending(&mut self) {
        self.status = PositionStatus::ExitPending;
        self.updated_at = Utc::now();
    }

    /// Terminally mark the position as failed-to-exit — the exit attempt ran and
    /// failed. Final: the position is never re-evaluated for exit again. Records
    /// the price (and time) at which the exit condition was met — i.e. the price
    /// it *would* have exited at had the sell/confirmation succeeded — so the row
    /// still carries a (hypothetical) PnL for analysis.
    pub fn mark_exit_failed(&mut self, exit_price: f64, exit_time: DateTime<Utc>) {
        self.exit_price = Some(exit_price);
        self.exit_time = Some(exit_time);
        self.status = PositionStatus::ExitFailed;
        self.updated_at = Utc::now();
    }

    /// Close the position with an exit fill. `exit_tx_signatures` are the
    /// signature(s) of this position's OWN sell leg(s) that cleared the balance
    /// (one for a single-shot sell; several when the sell-confirm loop retried /
    /// re-routed across migration).
    /// Flip status from PendingEntry to Holding when the on-chain fill is
    /// confirmed in-memory (the repo `update_entry` does the same atomically
    /// in the DB; this helper keeps the in-memory `Position` consistent so
    /// callers can mutate the value before handing it back to the cache).
    #[allow(dead_code)]
    pub fn mark_entry_filled(&mut self) {
        self.status = PositionStatus::Holding;
        self.updated_at = chrono::Utc::now();
    }

    pub fn close(
        &mut self,
        exit_price: f64,
        exit_tx_signatures: Vec<String>,
        exit_token_amount: f64,
        exit_time: DateTime<Utc>,
    ) {
        self.exit_price = Some(exit_price);
        self.exit_tx_signatures = exit_tx_signatures;
        self.exit_token_amount = Some(exit_token_amount);
        self.exit_time = Some(exit_time);
        self.status = PositionStatus::End;
        self.updated_at = Utc::now();
    }

    /// Whether this position is in the "holding index" (either awaiting fill or
    /// fully entered but not yet exiting). Used by the runtime cache and the
    /// exit guard to decide if a position is visible to the strategy hot path.
    pub fn is_in_holding_index(&self) -> bool {
        matches!(self.status, PositionStatus::Holding | PositionStatus::PendingEntry)
    }

    /// Calculate profit/loss percentage.
    pub fn pnl_percentage(&self) -> Option<f64> {
        match (self.exit_price, self.entry_price) {
            (Some(exit), Some(entry)) if entry != 0.0 => {
                Some(((exit - entry) / entry) * 100.0)
            }
            _ => None,
        }
    }

    /// The exit reason to display: the reason recorded at exit time when present,
    /// otherwise a best-effort fallback for legacy rows that closed before the
    /// `exit_reason` column existed (PnL sign for a clean close, the status for a
    /// failed one). `None` while the position is still open.
    pub fn exit_reason_or_derived(&self) -> Option<String> {
        if let Some(reason) = &self.exit_reason {
            return Some(reason.clone());
        }
        match self.status {
            PositionStatus::End => Some(
                if self.pnl_percentage().unwrap_or(0.0) >= 0.0 {
                    "TakeProfit"
                } else {
                    "StopLoss"
                }
                .to_string(),
            ),
            PositionStatus::ExitFailed => Some("ExitFailed".to_string()),
            PositionStatus::Holding | PositionStatus::PendingEntry | PositionStatus::ExitPending => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_position() -> Position {
        Position::new("mint".into(), "wallet".into(), "TPSL1".into(), Uuid::new_v4())
    }

    #[test]
    fn new_position_starts_pending_entry() {
        let p = make_position();
        assert_eq!(p.status, PositionStatus::PendingEntry);
    }

    #[test]
    fn pending_entry_round_trips_display_fromstr() {
        let s = PositionStatus::PendingEntry.to_string();
        assert_eq!(s, "PendingEntry");
        assert_eq!(s.parse::<PositionStatus>().unwrap(), PositionStatus::PendingEntry);
    }

    #[test]
    fn exit_reason_is_none_for_pending_entry() {
        let p = make_position();
        assert!(p.exit_reason_or_derived().is_none());
    }

    #[test]
    fn mark_entry_filled_transitions_to_holding() {
        let mut p = make_position();
        assert_eq!(p.status, PositionStatus::PendingEntry);
        p.mark_entry_filled();
        assert_eq!(p.status, PositionStatus::Holding);
    }

    #[test]
    fn is_in_holding_index_covers_both_pre_fill_statuses() {
        let mut p = make_position();
        assert!(p.is_in_holding_index(), "PendingEntry is in holding index");
        p.mark_entry_filled();
        assert!(p.is_in_holding_index(), "Holding is in holding index");
        p.mark_exit_pending();
        assert!(!p.is_in_holding_index(), "ExitPending is NOT in holding index");
    }
}
