use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a position held in a token.
/// A position is created when a buy rule is triggered, and closed when an exit rule is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: Uuid,
    /// Token mint address (SPL token address).
    pub mint_address: String,
    /// Wallet address that owns this position.
    pub wallet: String,
    /// Token program id used for this position (SPL legacy or Token-2022).
    pub token_program_id: Option<String>,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual `entry_*` fill. Set later on the
    /// arming path (the repo `update_target`), not at construction; `None` until
    /// armed (and for legacy rows / paths that never arm, e.g. backtest).
    /// `target_price` is the
    /// trigger trade's price, `target_token_amount` its **token** count,
    /// `target_time` its block time, `target_tx` its signature. SOL is never
    /// stored — derived at display as `price × tokens`. The gap vs. `entry_*` is
    /// derived later, not stored.
    pub target_price: Option<f64>,
    /// Raw token units (exact integer).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    /// Entry price (SOL per raw token unit) when the position was opened.
    pub entry_price: Option<f64>,
    /// Amount of tokens bought at entry — raw units (exact integer).
    pub entry_token_amount: Option<u64>,
    /// On-chain block time of the confirmed buy trade.
    pub entry_time: Option<DateTime<Utc>>,
    /// Transaction signature(s) that made up the entry fill. Single-leg today (one
    /// snipe buy), but a JSONB array so a scaled-in, multi-leg entry needs no
    /// schema change. Empty until the on-chain fill is adopted (per-signature
    /// attribution — each concurrent same-token position tracks its OWN buy tx, so
    /// the shared `(wallet, mint)` feed can never adopt another position's fill).
    pub entry_tx_signatures: Vec<String>,
    /// Exit price (SOL per raw token unit) when the position was closed.
    pub exit_price: Option<f64>,
    /// Amount of tokens sold at exit — raw units (exact integer).
    pub exit_token_amount: Option<u64>,
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
    /// tokens may already exist on-chain. Reached from `Arming` via the repo
    /// `mark_buy_submitted` the instant a send returns. Transitions
    /// to `Holding` via `update_entry` once the on-chain fill is adopted. In the
    /// holding index but not counted toward the cap (no entry yet). **Never
    /// reaped** — owned by the buy-recovery reaper, which adopts/waits/drops per
    /// its persisted `submitted_buy_signatures`.
    BuySubmitted,
    ExitPending,
    /// Terminal (for automation): the sell may have landed — or may still land — but
    /// the feed never confirmed the clear and the tx did not revert, so it is never
    /// auto-re-sold (double-sell risk). Alarmed for manual review (C1). Distinct from
    /// `ExitFailed`, which asserts nothing sold.
    ExitUnconfirmed,
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
            Self::ExitUnconfirmed => write!(f, "ExitUnconfirmed"),
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
            "ExitUnconfirmed" => Ok(Self::ExitUnconfirmed),
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
            mint_address: mint,
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

    pub fn close(
        &mut self,
        exit_price: f64,
        exit_tx_signatures: Vec<String>,
        exit_token_amount: u64,
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
                // price is SOL per RAW token unit; cast the raw-unit counts to f64 at
                // the multiply so the product is human SOL (not a 1e9-scaled value).
                let proceeds = exit_price * self.exit_token_amount.unwrap_or(0) as f64;
                Some(proceeds - entry_price * entry_tokens as f64)
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
            PositionStatus::ExitUnconfirmed => Some("ExitUnconfirmed".to_string()),
            PositionStatus::Holding
            | PositionStatus::Arming
            | PositionStatus::BuySubmitted
            | PositionStatus::ExitPending => None,
        }
    }
}

/// Wire view of a [`Position`] for the API + SSE stream (the "tpsl2" shape, which
/// carries the `target_*` snapshot). Lives in core next to `Position` so the core
/// SSE render bridge can emit it; `api::handlers::strategies::tpsl2_positions`
/// re-exports it.
///
/// SSOT NOTE: the frontend consumes this AND the sibling tpsl1 `PositionResponse`
/// (`hunter/live/.../strategies/positions.rs`) through ONE shared `RulePositionRecord`
/// type. The two are intentionally separate structs (per-strategy clones), but their
/// SERIALIZED field set must stay a consistent superset — the tpsl1 shape now also
/// emits `target_*` for parity. If you add a wire field to one, add it to both.
#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub mint_address: String,
    pub wallet: String,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual entry fill. `None` until armed.
    /// The gap vs. the `entry_*` columns is derived client-side, not stored.
    pub target_price: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    pub entry_price: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub entry_token_amount: Option<u64>,
    pub entry_time: Option<DateTime<Utc>>,
    /// First entry leg's signature (display/back-compat); empty until adopted.
    pub entry_tx: String,
    pub exit_price: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub exit_token_amount: Option<u64>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Last exit leg's signature (display/back-compat); `None` until a sell lands.
    pub exit_tx: Option<String>,
    /// All signatures that made up the entry fill (per-signature attribution, 1C).
    pub entry_tx_signatures: Vec<String>,
    /// All signatures that made up the exit fill (multi-leg: retries / re-routes).
    pub exit_tx_signatures: Vec<String>,
    pub pnl_percent: Option<f64>,
    /// Realized SOL PnL (`Position::pnl_sol`) — the canonical win/loss basis
    /// mirroring `StrategyPosition::is_win`/`positions_summary`; independent of
    /// `pnl_percent` so the two can be compared/reconciled on the frontend.
    pub pnl_sol: Option<f64>,
    pub status: String,
    pub strategy: String,
    /// Owning rule (`None` if the rule was deleted — `ON DELETE SET NULL`). Matches
    /// the sibling tpsl1 shape's `Option<Uuid>` so the two can't type-drift.
    pub rule_id: Option<Uuid>,
    /// Why the position exited ("TakeProfit", "StopLoss", "TrailingStop",
    /// "Stall", "TimeStop", "LiquidityExit"); `None` while still open.
    pub exit_reason: Option<String>,
    /// Owning run's monotonic sequence (`strategy_runs.run_seq`). Only populated
    /// by the run-history ("old runs") positions view — where it drives the run
    /// column + per-run banding; `None` on the current-run/live paths (single run,
    /// no need to distinguish) and on SSE deltas.
    pub run_seq: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Token symbol (row-owned identity; excluded from the shared `token` flatten).
    /// Empty until enriched by the paged handler.
    pub symbol: String,
    /// Token all-time-high price (`tokens_info`; row-owned, excluded from `token`).
    pub ath_price: Option<f64>,
    /// Full shared token enrichment (`name`, `market_cap`, `cu_price`, `trade_count`,
    /// `is_migrated`, …) — the same SSOT the Matched / Simulated / Sweep tables use,
    /// attached server-side so the positions table sorts/filters/searches on token
    /// columns with no client merge. Default (empty) on the SSE-delta path.
    #[serde(flatten)]
    pub token: crate::storage::token_enrichment::TokenEnrichment,
}

impl From<Position> for PositionResponse {
    fn from(p: Position) -> Self {
        let pnl_percent = p.pnl_percentage();
        let pnl_sol = p.pnl_sol();
        let exit_reason = p.exit_reason_or_derived();
        Self {
            id: p.id,
            mint_address: p.mint_address,
            wallet: p.wallet,
            target_price: p.target_price,
            target_token_amount: p.target_token_amount,
            target_time: p.target_time,
            target_tx: p.target_tx,
            entry_price: p.entry_price,
            entry_token_amount: p.entry_token_amount,
            entry_time: p.entry_time,
            // First entry leg / last exit leg for the single-address display columns.
            entry_tx: p.entry_tx_signatures.first().cloned().unwrap_or_default(),
            exit_price: p.exit_price,
            exit_token_amount: p.exit_token_amount,
            exit_time: p.exit_time,
            exit_tx: p.exit_tx_signatures.last().cloned(),
            entry_tx_signatures: p.entry_tx_signatures,
            exit_tx_signatures: p.exit_tx_signatures,
            pnl_percent,
            pnl_sol,
            status: p.status.to_string(),
            strategy: p.strategy,
            rule_id: Some(p.rule_id),
            exit_reason,
            // `Position` (legacy shape) carries no run_seq; the run-history handler
            // stamps it after construction from the run map.
            run_seq: None,
            created_at: p.created_at,
            updated_at: p.updated_at,
            // Enrichment attached by the paged handler; default on the SSE-delta path.
            symbol: String::new(),
            ath_price: None,
            token: Default::default(),
        }
    }
}

/// Adapt a unified [`StrategyPosition`] (the `strategy_positions` row) back into
/// the legacy [`Position`] wire shape. The SSE `tpsl_positions_changed` delta is
/// rendered through `PositionResponse::from(Position)` in the stream bridge, so
/// keeping this adapter lets the live edge emit position deltas under the new
/// schema **without changing the frontend wire contract** (the rendered JSON is
/// byte-for-byte the old shape). `rule_id` is non-optional here — a live position
/// always carries one; the `None` fallback (`Uuid::nil`) only guards malformed
/// rows. `strategy` carries the canonical `strategy_id`; the frontend uses it
/// only as a trade-mode fallback when the richer rule snapshot is absent.
impl From<&crate::models::StrategyPosition> for Position {
    fn from(p: &crate::models::StrategyPosition) -> Self {
        Self {
            id: p.id,
            mint_address: p.mint_address.clone(),
            wallet: p.wallet.clone(),
            token_program_id: p.token_program_id.clone(),
            target_price: p.target_price,
            target_token_amount: p.target_token_amount,
            target_time: p.target_time,
            target_tx: p.target_tx.clone(),
            entry_price: p.entry_price,
            entry_token_amount: p.entry_token_amount,
            entry_time: p.entry_time,
            entry_tx_signatures: p.entry_tx_sigs(),
            exit_price: p.exit_price,
            exit_token_amount: p.exit_token_amount,
            exit_time: p.exit_time,
            exit_tx_signatures: p.exit_tx_sigs(),
            submitted_buy_signatures: p.submitted_buy_signatures.clone(),
            status: p.status.parse().unwrap_or(PositionStatus::Holding),
            strategy: p.strategy_id.clone(),
            rule_id: p.rule_id.unwrap_or_default(),
            exit_reason: p.exit_reason.clone(),
            created_at: p.created_at,
            updated_at: p.updated_at,
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

    /// A clean profitable exit: win, positive SOL + %, closed.
    #[test]
    fn pnl_and_win_for_clean_profitable_exit() {
        let mut p = make_position();
        p.entry_price = Some(1.0);
        p.entry_token_amount = Some(100);
        p.close(2.0, vec!["sig".into()], 100, Utc::now());
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
        loss.entry_token_amount = Some(100);
        loss.close(0.5, vec!["sig".into()], 100, Utc::now());
        assert!(loss.is_closed() && !loss.is_win());
        assert_eq!(loss.pnl_sol(), Some(-50.0));

        let mut breakeven = make_position();
        breakeven.entry_price = Some(1.0);
        breakeven.entry_token_amount = Some(100);
        breakeven.close(1.0, vec!["sig".into()], 100, Utc::now());
        assert!(!breakeven.is_win(), "0% is a loss, not a win");
    }

    /// A failed exit that sold nothing books a SOL loss (proceeds fall back to 0)
    /// and is never a win, even though `ExitFailed` is terminal/closed.
    #[test]
    fn failed_exit_is_closed_loss_with_sol_loss() {
        let mut p = make_position();
        p.entry_price = Some(1.0);
        p.entry_token_amount = Some(100);
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
        holding.entry_token_amount = Some(100);
        holding.status = PositionStatus::Holding;
        assert!(!holding.is_closed(), "Holding is not closed");
        assert_eq!(holding.pnl_sol(), None, "no exit price yet");
    }

    #[test]
    fn is_in_holding_index_covers_all_pre_fill_statuses() {
        let mut p = make_position();
        assert!(p.is_in_holding_index(), "Arming is in holding index");
        p.status = PositionStatus::BuySubmitted;
        assert!(p.is_in_holding_index(), "BuySubmitted is in holding index");
        p.status = PositionStatus::Holding;
        assert!(p.is_in_holding_index(), "Holding is in holding index");
        p.mark_exit_pending();
        assert!(!p.is_in_holding_index(), "ExitPending is NOT in holding index");
    }
}
