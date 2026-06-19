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
    /// trigger trade's price, `target_amount` its SOL amount, `target_time` its
    /// block time, `target_tx` its signature. The gap vs. `entry_*` is derived
    /// later, not stored.
    pub target_price: Option<f64>,
    pub target_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    /// Entry price (SOL per token) when the position was opened.
    pub entry_price: Option<f64>,
    /// Amount of tokens bought at entry.
    pub entry_amount: Option<f64>,
    /// On-chain block time of the confirmed buy trade.
    pub entry_time: Option<DateTime<Utc>>,
    /// Transaction signature of the buy transaction.
    pub entry_tx: String,
    /// Exit price (SOL per token) when the position was closed.
    pub exit_price: Option<f64>,
    /// Amount of tokens sold at exit.
    pub exit_amount: Option<f64>,
    /// On-chain block time of the confirmed sell trade.
    pub exit_time: Option<DateTime<Utc>>,
    /// Transaction signature of the sell transaction.
    pub exit_tx: Option<String>,
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
            target_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: None,
            entry_amount: None,
            entry_time: None,
            entry_tx: String::new(),
            exit_price: None,
            exit_amount: None,
            exit_time: None,
            exit_tx: None,
            status: PositionStatus::Holding,
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
        self.target_amount = Some(amount);
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

    /// Close the position with an exit trade.
    pub fn close(&mut self, exit_price: f64, exit_tx: String, exit_amount: f64, exit_time: DateTime<Utc>) {
        self.exit_price = Some(exit_price);
        self.exit_tx = Some(exit_tx);
        self.exit_amount = Some(exit_amount);
        self.exit_time = Some(exit_time);
        self.status = PositionStatus::End;
        self.updated_at = Utc::now();
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
            PositionStatus::Holding | PositionStatus::ExitPending => None,
        }
    }
}
