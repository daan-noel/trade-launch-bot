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
    /// Entry price (SOL per token) when the position was opened.
    pub entry_price: f64,
    /// Exit price (SOL per token) when the position was closed.
    pub exit_price: Option<f64>,
    /// Transaction signature of the buy transaction.
    pub entry_tx: String,
    /// Transaction signature of the sell transaction.
    pub exit_tx: Option<String>,
    /// "Holding" — still owns tokens | "End" — all tokens sold.
    pub status: PositionStatus,
    /// Strategy name (e.g., "TPSL").
    pub strategy: String,
    /// Rule ID from the strategy rules table that triggered this position.
    pub rule_id: Uuid,
    /// Amount of tokens bought at entry.
    pub entry_amount: f64,
    /// Amount of tokens sold at exit.
    pub exit_amount: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PositionStatus {
    Holding,
    End,
}

impl std::fmt::Display for PositionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Holding => write!(f, "Holding"),
            Self::End => write!(f, "End"),
        }
    }
}

impl std::str::FromStr for PositionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Holding" => Ok(Self::Holding),
            "End" => Ok(Self::End),
            _ => Err(format!("Unknown status: {}", s)),
        }
    }
}

impl Position {
    pub fn new(
        mint: String,
        wallet: String,
        entry_price: f64,
        entry_tx: String,
        strategy: String,
        rule_id: Uuid,
        entry_amount: f64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            mint,
            wallet,
            entry_price,
            exit_price: None,
            entry_tx,
            exit_tx: None,
            status: PositionStatus::Holding,
            strategy,
            rule_id,
            entry_amount,
            exit_amount: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Close the position with an exit trade.
    pub fn close(&mut self, exit_price: f64, exit_tx: String, exit_amount: f64) {
        self.exit_price = Some(exit_price);
        self.exit_tx = Some(exit_tx);
        self.exit_amount = Some(exit_amount);
        self.status = PositionStatus::End;
        self.updated_at = Utc::now();
    }

    /// Calculate profit/loss percentage.
    pub fn pnl_percentage(&self) -> Option<f64> {
        self.exit_price
            .map(|ep| ((ep - self.entry_price) / self.entry_price) * 100.0)
    }
}
