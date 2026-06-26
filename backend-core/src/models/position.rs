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
    /// Signature(s) of every snipe buy this position **submitted** (one per
    /// attempt; the buy loop can send up to `BUY_MAX_ATTEMPTS` times). Written
    /// the instant a send returns — *before* the fill is confirmed — so that a
    /// crash/restart in the send→record gap can recover the entry by checking
    /// these signatures against the trade feed (`BuySubmitted` recovery reaper).
    /// Empty until the first buy is sent; real positions only (paper sends no
    /// buys). Distinct from `entry_tx_signatures`, which records only the *fill*.
    pub submitted_buy_signatures: Vec<String>,
    /// "Arming" — matched a rule, watching the live feed for the scalp trigger;
    /// no buy sent, no SOL committed (reapable) | "BuySubmitted" — buy
    /// signed/sent, awaiting the fill; tokens may exist on-chain (must be
    /// recovered, never reaped) | "Holding" — owns tokens, exit not yet
    /// triggered | "ExitPending" — exit triggered, sell/confirmation in flight |
    /// "End" — exited cleanly | "ExitFailed" — terminal: the exit attempt ran
    /// and failed.
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
    /// Matched a rule and watching the live feed for the scalp entry trigger
    /// (`await_scalp_entry_signal`) — **no buy sent, no SOL committed**. The
    /// initial state of every position (`Position::new`). In the holding index
    /// (so by-mint lookups find it) but not counted toward the per-rule cap;
    /// excluded from frontend summary tallies. Safe to reap as an orphan once
    /// stale — nothing was bought. (Was `PendingEntry`.)
    Arming,
    /// A buy has been **signed and sent**, but its fill is not yet recorded —
    /// tokens may already exist on-chain. Reached from `Arming` via
    /// [`Position::mark_buy_submitted`] the instant a send returns. Transitions
    /// to `Holding` via `update_entry` once the on-chain fill is adopted. In the
    /// holding index but not counted toward the cap (no entry yet). **Never
    /// reaped** — owned by the buy-recovery reaper, which adopts/waits/drops per
    /// its persisted `submitted_buy_signatures`.
    BuySubmitted,
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
            Self::Arming => write!(f, "Arming"),
            Self::BuySubmitted => write!(f, "BuySubmitted"),
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
            "Arming" => Ok(Self::Arming),
            "BuySubmitted" => Ok(Self::BuySubmitted),
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
            submitted_buy_signatures: Vec::new(),
            exit_price: None,
            exit_token_amount: None,
            exit_time: None,
            exit_tx_signatures: Vec::new(),
            status: PositionStatus::Arming,
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

    /// Record a submitted snipe-buy signature and flip the status to
    /// `BuySubmitted` (the durable "buy in flight" marker). Appends to
    /// `submitted_buy_signatures` so every attempt is recoverable. In-memory
    /// mutator parallel to the repo's `mark_buy_submitted`; idempotent on the
    /// status (re-marking an already-`BuySubmitted` position just appends the
    /// new attempt's signature).
    #[allow(dead_code)]
    pub fn mark_buy_submitted(&mut self, signature: String) {
        self.submitted_buy_signatures.push(signature);
        self.status = PositionStatus::BuySubmitted;
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
        matches!(
            self.status,
            PositionStatus::Holding | PositionStatus::Arming | PositionStatus::BuySubmitted
        )
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

    /// Realized SOL profit/loss: exit proceeds minus entry cost. `None` until the
    /// position has both an entry fill and an exit price. A terminal `ExitFailed`
    /// row may carry no `exit_token_amount` (nothing — or only part — actually
    /// sold); proceeds then fall back to `0`, booking the lost bag as a SOL loss.
    /// Mirrors the warm-up aggregate's `COALESCE(exit_token_amount, 0)` so the
    /// live counter and the startup query agree.
    pub fn pnl_sol(&self) -> Option<f64> {
        match (self.entry_price, self.entry_token_amount, self.exit_price) {
            (Some(entry_price), Some(entry_tokens), Some(exit_price)) => {
                let proceeds = exit_price * self.exit_token_amount.unwrap_or(0.0);
                Some(proceeds - entry_price * entry_tokens)
            }
            _ => None,
        }
    }

    /// Whether this position is terminally closed *with money deployed* — an
    /// entered position that reached `End` (clean exit) or `ExitFailed`. The
    /// per-rule performance stats accumulate exactly on the transition into this
    /// state. `Arming`/`BuySubmitted`/`Holding`/`ExitPending` are not closed; an
    /// unentered row (no `entry_price`) is never counted.
    pub fn is_closed(&self) -> bool {
        self.entry_price.is_some()
            && matches!(self.status, PositionStatus::End | PositionStatus::ExitFailed)
    }

    /// Whether this closed position is a win: a *clean* exit (`End`) that sold
    /// above entry. A failed exit (`ExitFailed`) is never a win — the bag wasn't
    /// realized — so it always falls to the loss bucket. Breakeven counts as a
    /// loss (`> 0`, not `>= 0`).
    pub fn is_win(&self) -> bool {
        self.status == PositionStatus::End && self.pnl_percentage().is_some_and(|p| p > 0.0)
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
            PositionStatus::Holding
            | PositionStatus::Arming
            | PositionStatus::BuySubmitted
            | PositionStatus::ExitPending => None,
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
    fn new_position_starts_arming() {
        let p = make_position();
        assert_eq!(p.status, PositionStatus::Arming);
    }

    #[test]
    fn statuses_round_trip_display_fromstr() {
        for status in [PositionStatus::Arming, PositionStatus::BuySubmitted] {
            let s = status.to_string();
            assert_eq!(s.parse::<PositionStatus>().unwrap(), status);
        }
        assert_eq!(PositionStatus::Arming.to_string(), "Arming");
        assert_eq!(PositionStatus::BuySubmitted.to_string(), "BuySubmitted");
    }

    #[test]
    fn exit_reason_is_none_for_arming() {
        let p = make_position();
        assert!(p.exit_reason_or_derived().is_none());
    }

    #[test]
    fn mark_buy_submitted_appends_sig_and_flips_status() {
        let mut p = make_position();
        assert_eq!(p.status, PositionStatus::Arming);
        p.mark_buy_submitted("sig-1".into());
        assert_eq!(p.status, PositionStatus::BuySubmitted);
        assert_eq!(p.submitted_buy_signatures, vec!["sig-1".to_string()]);
        // A second send (retry) just appends — still BuySubmitted, still in index.
        p.mark_buy_submitted("sig-2".into());
        assert_eq!(p.submitted_buy_signatures, vec!["sig-1".to_string(), "sig-2".to_string()]);
        assert!(p.is_in_holding_index());
    }

    #[test]
    fn mark_entry_filled_transitions_to_holding() {
        let mut p = make_position();
        assert_eq!(p.status, PositionStatus::Arming);
        p.mark_entry_filled();
        assert_eq!(p.status, PositionStatus::Holding);
    }

    /// A clean profitable exit: win, positive SOL + %, closed.
    #[test]
    fn pnl_and_win_for_clean_profitable_exit() {
        let mut p = make_position();
        p.entry_price = Some(1.0);
        p.entry_token_amount = Some(100.0);
        p.close(2.0, vec!["sig".into()], 100.0, Utc::now());
        assert!(p.is_closed());
        assert!(p.is_win());
        assert_eq!(p.pnl_percentage(), Some(100.0));
        assert_eq!(p.pnl_sol(), Some(100.0)); // 2*100 - 1*100
    }

    /// A losing exit (sold below entry) is closed but not a win; breakeven also
    /// falls to the loss side (`> 0`, not `>= 0`).
    #[test]
    fn loss_and_breakeven_are_not_wins() {
        let mut loss = make_position();
        loss.entry_price = Some(1.0);
        loss.entry_token_amount = Some(100.0);
        loss.close(0.5, vec!["sig".into()], 100.0, Utc::now());
        assert!(loss.is_closed() && !loss.is_win());
        assert_eq!(loss.pnl_sol(), Some(-50.0));

        let mut breakeven = make_position();
        breakeven.entry_price = Some(1.0);
        breakeven.entry_token_amount = Some(100.0);
        breakeven.close(1.0, vec!["sig".into()], 100.0, Utc::now());
        assert!(!breakeven.is_win(), "0% is a loss, not a win");
    }

    /// A failed exit that sold nothing books a SOL loss (proceeds fall back to 0)
    /// and is never a win, even though `ExitFailed` is terminal/closed.
    #[test]
    fn failed_exit_is_closed_loss_with_sol_loss() {
        let mut p = make_position();
        p.entry_price = Some(1.0);
        p.entry_token_amount = Some(100.0);
        p.mark_exit_failed(0.0, Utc::now()); // paper total loss: no exit tokens
        assert!(p.is_closed());
        assert!(!p.is_win());
        assert_eq!(p.pnl_sol(), Some(-100.0)); // 0 proceeds - 1*100 entry cost
    }

    /// An un-entered or still-open position is neither closed nor priced.
    #[test]
    fn open_and_unentered_positions_are_not_closed() {
        let pending = make_position(); // Arming, no entry
        assert!(!pending.is_closed());
        assert_eq!(pending.pnl_sol(), None);

        let mut holding = make_position();
        holding.entry_price = Some(1.0);
        holding.entry_token_amount = Some(100.0);
        holding.mark_entry_filled();
        assert!(!holding.is_closed(), "Holding is not closed");
        assert_eq!(holding.pnl_sol(), None, "no exit price yet");
    }

    #[test]
    fn is_in_holding_index_covers_all_pre_fill_statuses() {
        let mut p = make_position();
        assert!(p.is_in_holding_index(), "Arming is in holding index");
        p.mark_buy_submitted("sig".into());
        assert!(p.is_in_holding_index(), "BuySubmitted is in holding index");
        p.mark_entry_filled();
        assert!(p.is_in_holding_index(), "Holding is in holding index");
        p.mark_exit_pending();
        assert!(!p.is_in_holding_index(), "ExitPending is NOT in holding index");
    }
}
