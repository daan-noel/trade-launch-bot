use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::strategies::kernel::weighted_return_pct;

/// A configured rule for the generic fingerprint + metrics engine. Backs the
/// `strategy_rules` table (0004 redesign schema). Columns say *how* the rule
/// trades; `params` (JSONB) says *when* — strict `take_profit`/`stop_loss` plus
/// `entry`/`exit` metric-condition groups, parsed/validated by
/// [`crate::strategies::rule_params::RuleParams`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRule {
    pub id: Uuid,
    /// Human-facing rule label.
    pub rule_name: String,
    /// The token-creation shape this rule arms on (`fingerprints` row).
    pub fingerprint_id: Uuid,
    /// Execution mode: `paper` or `real`.
    pub trade_mode: String,
    /// Whether the rule is eligible to fire (Active/Idle live arming).
    pub is_active: bool,
    /// Soft-archive flag. Disabled rules stay in the DB but are hidden from
    /// default lists and cannot be activated until re-enabled.
    pub is_enabled: bool,
    /// Buy size per fired token — exact lamports at rest.
    pub buy_amount_lamports: i64,
    /// Cap on concurrently-open tokens.
    pub max_concurrent_tokens: i64,
    /// Cap on total tokens across the rule's lifetime (0 = unlimited).
    pub max_total_tokens: i64,
    /// TP/SL + entry/exit metric conditions as JSON (redesign plan §5 shape).
    pub params: Value,
    /// Free-form labels for slicing the Rules board. **Presentational only** —
    /// never identity (unlike `params`), never read by the engine. Canonical
    /// shape is owned by [`crate::strategies::rules::normalize_tags`].
    /// `#[serde(default)]` so a pre-0002 stored/cached rule JSON still decodes.
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyRule {
    /// Buy size as human SOL (`f64`) — display/API convenience over the exact
    /// lamports column.
    pub fn buy_amount_sol(&self) -> f64 {
        crate::config::constants::lamports_to_sol(self.buy_amount_lamports)
    }
}

/// One execution of a rule (real or paper). Backs the `strategy_runs` table.
/// `run_seq` is monotonic per `(rule_id, mode)`; `params_snapshot` freezes the
/// rule params at launch so later rule edits don't rewrite history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRun {
    pub id: Uuid,
    pub strategy_id: String,
    /// Owning rule (None if the rule was deleted — `ON DELETE SET NULL`).
    pub rule_id: Option<Uuid>,
    /// Execution mode: `real` or `paper`.
    pub mode: String,
    /// Monotonic sequence per `(rule_id, mode)`.
    pub run_seq: i64,
    /// `Running` | `Finished` | `Stopped` | `Cancelled`.
    pub status: String,
    /// Frozen copy of the rule params at launch.
    pub params_snapshot: Value,
    pub max_total_tokens: Option<i64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Rolled-up performance metrics for a single run. Backs the
/// `strategy_run_metrics` table (1:1 with `strategy_runs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRunMetrics {
    pub run_id: Uuid,
    pub rolled_up_at: DateTime<Utc>,
    pub n_fired: i32,
    pub n_open: i32,
    pub n_closed: i32,
    pub win_rate: f32,
    pub total_pnl_sol: f32,
    pub expectancy_sol: f32,
    pub mean_pnl_pct: f32,
    pub median_pnl_pct: f32,
    pub p90_pnl_pct: f32,
    pub best_pnl_pct: f32,
    pub worst_pnl_pct: f32,
    pub std_pnl_pct: f32,
    pub profit_factor: Option<f32>,
    pub avg_holding_secs: f32,
    pub median_holding_secs: f32,
    pub n_exit_take_profit: i32,
    pub n_exit_stop_loss: i32,
    pub n_exit_trailing: i32,
    pub n_exit_stall: i32,
    pub n_exit_time: i32,
    pub n_exit_liquidity: i32,
    /// Analysis-only death-close (`ExitCode::Dead`); always 0 on a live rollup.
    pub n_exit_dead: i32,
    /// Generic-engine metric-condition exits — where **every** exit of a redesigned
    /// rule lands, since it has no tpsl/swing ladder. Without this column a live
    /// run's histogram read as all-zero next to a non-zero `n_closed` (mig 0004).
    pub n_exit_metrics: i32,
    /// Operator-forced closes (Console sell / Stop / Stop All) and any closed row
    /// with an unrecognized label.
    pub n_exit_manual: i32,
    /// Closed because the token graduated off the bonding curve.
    pub n_exit_migrated: i32,
    pub n_exit_open: i32,
}

/// Run-wide (or rule-wide) position aggregates for the strategy page's
/// **Positions Summary** panel. Computed server-side in SQL over the *entire*
/// population (never a page), using the same win/closed/open semantics as the
/// per-rule runtime counters ([`StrategyPosition::is_win`] etc.) so the summary
/// panel and the strategy-table row always agree.
///
/// SOL fields are human SOL (f64) — the SQL sums lamports and the repo divides.
/// A "win" is a clean `End` exit with positive realized SOL; every other closed
/// position (loss or breakeven) is a loss. `tokens` counts entered positions (a
/// real entry landed); `open` counts open-partition rows (everything not
/// `End`/`EntryFailed` — including stuck/unconfirmed exits still holding SOL).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionsSummary {
    /// Entered positions (a real entry fill landed) — the summary's "Tokens".
    pub tokens: i64,
    /// Open-partition rows (not `End`/`EntryFailed`).
    pub open: i64,
    /// Clean `End` exits with positive realized SOL.
    pub win: i64,
    /// Every other closed position (loss/breakeven).
    pub loss: i64,
    /// Closed positions = `win + loss` (the win-rate denominator).
    pub closed: i64,
    /// `win / closed * 100` (0 when nothing closed).
    pub win_rate: f64,
    /// **Canonical return %** — capital-weighted realized return over the closed
    /// positions: `total_pnl_sol / closed_entry_sol × 100`, via
    /// [`weighted_return_pct`]. NOT a mean of per-trade percents: a rule whose
    /// buy size changed (`buy_amount_lamports` is editable mid-run) must weight
    /// a 1.0 ◎ trade above a 0.05 ◎ one. Sign-locked to `total_pnl_sol`.
    /// 0 when nothing closed.
    pub return_pct: f64,
    /// Sum of realized SOL PnL across closed positions (human SOL).
    pub total_pnl_sol: f64,
    /// **Unrealized** counterpart to `total_pnl_sol`: the sum of still-open
    /// positions' mark-to-current-price PnL, priced through the same
    /// [`CostModel`](crate::strategies::kernel::CostModel) round-trip the sim and
    /// the sweep use, against the live token cache's `current_price`. Reported
    /// alongside the realized total, never folded into it — so a rule holding its
    /// losers open can't read as profitable (parity plan B11, matching
    /// [`RunMetrics::open_pnl_sol`](crate::strategies::kernel::RunMetrics)).
    ///
    /// `0.0` when nothing is open, or when no open position has a cached price yet
    /// (a just-entered token before its first post-entry trade).
    pub open_pnl_sol: f64,
    /// Sum of entry cost across entered positions (human SOL). Includes the
    /// still-open ones, so it is **not** the denominator of a realized return —
    /// use `closed_entry_sol` for that.
    pub total_entry_sol: f64,
    /// Entry cost of the **closed** positions only (human SOL) — the exact
    /// denominator behind `return_pct`, shipped so a caller aggregating several
    /// scopes (the Rules total tile) can re-weight by capital instead of by
    /// trade count. Dividing `total_pnl_sol` by `total_entry_sol` instead
    /// understates the return by the open positions' share of capital.
    pub closed_entry_sol: f64,
    /// Entry cost still deployed in open (holding) positions (human SOL).
    pub total_holding_sol: f64,
    /// Sum of positive realized PnL across winning closes (human SOL).
    pub total_gains_sol: f64,
    /// Sum of |realized PnL| across losing closes (human SOL, positive).
    pub total_losses_sol: f64,
    /// Mean hold time (seconds) across closed positions (0 when none).
    pub avg_hold_secs: f64,
    /// Arithmetic **sum** of the closed positions' [`StrategyPosition::pnl_pct`] —
    /// the History table's PnL% column added up.
    ///
    /// Every position counts as one equal unit here, so this is a *tally*, not a
    /// return: it grows with trade count (100 closes at +1% reads `+100%`) and two
    /// cohorts of different size can't be compared by it. `return_pct` remains the
    /// canonical figure — see
    /// [pnl-percent-definition.md](../../../docs/plans/strategies/pnl-percent-definition.md).
    /// Shipped so a reader who sums the PnL% column by hand lands on the same
    /// number the strip shows, exactly past the page and the client row cap.
    pub sum_pnl_pct: f64,
    /// Best closed PnL % (`None` when nothing closed).
    pub best_pct: Option<f64>,
    /// Worst closed PnL % (`None` when nothing closed).
    pub worst_pct: Option<f64>,
    /// Entered tokens that graduated off the bonding curve to AMM
    /// (`tokens_info.is_migrated`). A token-quality signal independent of how the
    /// rule exited — a token can migrate and still be closed at a stop. Counted
    /// among `tokens` (entered positions), so `migrated <= tokens`.
    pub migrated: i64,
    /// How the closed positions left, by `exit_reason`.
    pub exits: ExitReasonCounts,
}

/// Closed-position counts keyed by [`StrategyPosition::exit_reason`].
///
/// Deliberately **not** exhaustive of `closed`: a position can close with no
/// reason at all, or with a reason minted after this list. The frontend reconciles the difference against
/// `closed` into a visible `Other` slice rather than having the parts silently
/// fail to sum to the whole — so an unmodelled reason shows up as a number to
/// investigate instead of quietly skewing the mix.
///
/// Counted in the same single SQL pass as the rest of [`PositionsSummary`], so
/// this costs no extra round-trip.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExitReasonCounts {
    pub take_profit: i64,
    pub stop_loss: i64,
    /// The generic engine's single metric-condition exit (total).
    /// Equals `metrics_win + metrics_loss` — kept so callers that only need
    /// the reason count don't have to sum the PnL split.
    pub metrics: i64,
    /// Metric exits with positive realized SOL (`is_win`).
    #[serde(default)]
    pub metrics_win: i64,
    /// Metric exits that are not wins (loss or break-even).
    #[serde(default)]
    pub metrics_loss: i64,
    /// Death-close: liquidity gone and the token went silent.
    pub dead: i64,
    /// Operator-initiated close (a "Sell ALL" / rule stop) — live-only; the
    /// analysis kernel has no manual close, so this has no `RunMetrics` peer.
    pub manual: i64,
    /// The legacy tpsl/swing ladder reasons, retained so a legacy live rule's
    /// breakdown is still complete until those strategies are deleted.
    pub trailing: i64,
    pub stall: i64,
    pub time: i64,
    pub liquidity: i64,
}

/// Exit-side realized SOL for a position — the ONE decider of which exit figure
/// counts: the running sell-leg aggregate (`exit_sol_total`) once any sell leg
/// landed (`sold_token_amount > 0`, scale-out aware), else the stamped single-leg
/// `exit_sol` (legacy). Shared by [`StrategyPosition::realized_pnl_sol`] and the
/// repo's closes-series projection so the preference logic can't drift.
pub fn realized_exit_sol(
    sold_token_amount: u64,
    exit_sol_total: f64,
    exit_sol: Option<f64>,
) -> Option<f64> {
    if sold_token_amount > 0 {
        Some(exit_sol_total)
    } else {
        exit_sol
    }
}

/// A single position lifecycle within a run. Backs the `strategy_positions`
/// table. JSONB signature lists are `serde_json::Value`; the Postgres `TEXT[]`
/// `submitted_buy_signatures` maps to `Vec<String>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyPosition {
    pub id: Uuid,
    pub run_id: Uuid,
    pub strategy_id: String,
    pub rule_id: Option<Uuid>,
    /// Execution mode: `real` or `paper`.
    pub mode: String,
    pub mint_address: String,
    pub wallet: String,
    pub token_program_id: Option<String>,
    /// The wallet's token account address for `mint` (base58). Persisted after the
    /// entry fill so subsequent buys reuse one account and the sell reads it from
    /// the row — no in-memory-cache dependency, survives restarts. `None` until the
    /// first fill, or on legacy rows predating this column (callers fall back to the
    /// cache-first resolver).
    pub token_account: Option<String>,
    // Target (arming) snapshot.
    pub target_price: Option<f64>,
    /// Raw token units (exact integer).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    // Entry fill.
    pub entry_price: Option<f64>,
    /// Raw token units (exact integer).
    pub entry_token_amount: Option<u64>,
    /// Human SOL (f64) in the model; stored as exact lamports (BIGINT) in the column.
    pub entry_sol: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub entry_tx_signatures: Value,
    // Exit fill.
    pub exit_price: Option<f64>,
    /// Raw token units (exact integer).
    pub exit_token_amount: Option<u64>,
    /// Human SOL (f64) in the model; stored as exact lamports (BIGINT) in the column.
    pub exit_sol: Option<f64>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_tx_signatures: Value,
    /// Running sum of confirmed sell-leg raw token units (mig 0018). Ledger is
    /// authority; this is the denormalized cache for list/sizing.
    #[serde(default)]
    pub sold_token_amount: u64,
    /// Running sum of confirmed sell-leg SOL (human), from `exit_sol_lamports_total`.
    #[serde(default)]
    pub exit_sol_total: f64,
    /// Next scale-out stage index (`0` = pre-first partial / legacy).
    #[serde(default)]
    pub scale_stage: u8,
    /// Raw submitted buy signatures (`TEXT[]`).
    pub submitted_buy_signatures: Vec<String>,
    /// `BuySubmitted` | `Holding` | `ExitPending` | `ExitUnconfirmed` |
    /// `ExitStuck` | `End` | `EntryFailed`.
    ///
    /// Open partition: everything but `End`/`EntryFailed` — a row with SOL still
    /// in it (`ExitStuck`, `ExitUnconfirmed`) is OPEN, never "recent closed".
    pub status: String,
    pub exit_reason: Option<String>,
    /// `bot` (engine-armed) | `manual` (operator buy via the Console).
    #[serde(default = "default_origin")]
    pub origin: String,
    /// Manual-position exit config (`{"tp_pct": .., "sl_pct": ..}`). `None` on bot
    /// rows and on tracked-only manual rows (no auto-exit of any kind).
    #[serde(default)]
    pub manual_exit: Option<Value>,
    /// Reaper redrive counter (mig 0012) — **read-only** in this model: the repo's
    /// `insert_position`/`update_position` never write it (only `bump_exit_redrive`
    /// / `set_exit_parked` / `unpark_exit` mutate it), so a full-row engine write
    /// can never clobber the reaper's state.
    #[serde(default)]
    pub exit_redrive_count: i32,
    /// Parked ⇒ the reaper stopped auto-redriving this `ExitStuck` bag (cap hit);
    /// read-only here for the same reason as `exit_redrive_count`.
    #[serde(default)]
    pub exit_parked: bool,
    /// Cause of the most recent buy attempt that did NOT fill — the send error or
    /// the on-chain Anchor code (mig 0017). Read-only in this model for the same
    /// reason as `exit_redrive_count`: the executor writes it at the moment of
    /// failure (`note_last_entry_error`, the ONE writer) and the sink's full-row
    /// terminal write lands after, so a shared write path would clobber it.
    ///
    /// Not `EntryFailed`-only and never cleared on success: on a `Holding` row it
    /// is the history of the attempts it took to get in. This is what tells a
    /// slippage revert (6002/6042 ⇒ tune the buy floor) from a structural one
    /// without pulling logs off the box.
    #[serde(default)]
    pub last_entry_error: Option<String>,
    /// Derived at DB-read time (not a column): a real `BuySubmitted` older than
    /// the review window with no adopted fill (B3) — needs a manual Verify. The
    /// ONE derivation site is the repo's row mapping; the UI renders it, never
    /// infers it from timestamps.
    #[serde(default)]
    pub needs_review: bool,
    pub extra: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyPosition {
    /// In the runtime holding index: buy in flight or held. These are the states
    /// the exit gate and fill-adopt path scan by mint.
    pub fn is_in_holding_index(&self) -> bool {
        matches!(self.status.as_str(), "BuySubmitted" | "Holding")
    }

    /// Fully entered and currently held (SOL deployed, not yet exiting).
    pub fn is_holding(&self) -> bool {
        self.status == "Holding"
    }

    /// Terminally closed — a confirmed exit or a buy that never filled. A stuck
    /// or unconfirmed exit is NOT closed (the bag may still be held).
    pub fn is_closed(&self) -> bool {
        matches!(self.status.as_str(), "End" | "EntryFailed")
    }

    /// A real entry landed (SOL deployed) — the gate for the cap counters.
    pub fn is_entered(&self) -> bool {
        self.entry_price.is_some()
    }

    /// Realized SOL PnL once closed. Exit-side preference via
    /// [`realized_exit_sol`] (the ONE decider); mirrors
    /// `strategy_position_pnl.realized_pnl_sol`.
    pub fn realized_pnl_sol(&self) -> Option<f64> {
        let entry = self.entry_sol?;
        let exit = realized_exit_sol(self.sold_token_amount, self.exit_sol_total, self.exit_sol)?;
        Some(exit - entry)
    }

    /// Realized PnL as a percent of the SOL actually **deployed** —
    /// `realized_pnl_sol / entry_sol × 100`, i.e. the per-position grain of the
    /// canonical [`weighted_return_pct`]. Mirrors `strategy_position_pnl.pnl_pct`
    /// and the repo's `PNL_PCT_SQL` (the sort/filter expression).
    ///
    /// This is a **money** return, not a price return. It is specifically **not**
    /// `(exit_price - entry_price) / entry_price`, which has two defects:
    ///
    /// * It charged no execution cost. At 125 bps/leg plus the fixed tip a round
    ///   trip needs roughly a **+4% price move just to break even**, so every
    ///   trade between 0% and break-even rendered a green % next to a red ◎ —
    ///   the exact `+%`/`−◎` contradiction [`weighted_return_pct`] was
    ///   introduced to kill on the aggregate surfaces.
    /// * It read `exit_price`, which stamps only the **last** sell leg, while
    ///   [`Self::realized_pnl_sol`] sums **every** leg via [`realized_exit_sol`].
    ///   On a scale-out the two headline numbers described different trades.
    ///
    /// Sign-locked to [`Self::realized_pnl_sol`] by construction: the
    /// denominator is capital, which is always positive.
    pub fn pnl_pct(&self) -> Option<f64> {
        let entry_sol = self.entry_sol.filter(|e| *e > 0.0)?;
        Some(weighted_return_pct(self.realized_pnl_sol()?, entry_sol))
    }

    /// A clean `End` exit that realized positive SOL — the win/loss classifier the
    /// per-rule closed-stats counters use (everything else is a loss).
    pub fn is_win(&self) -> bool {
        self.status == "End" && self.realized_pnl_sol().map(|p| p > 0.0).unwrap_or(false)
    }

    // ── Lifecycle ctor + mutators (the unified-schema twin of the old `Position`
    //    in-memory API; pure, no DB) ───────────────────────────────────────────

    /// A fresh `BuySubmitted` position within `run_id` (no fills yet). Mode/strategy
    /// are copied from the owning rule; `wallet` is the bot wallet (real) or a
    /// sentinel (paper).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: Uuid,
        strategy_id: String,
        rule_id: Uuid,
        mode: String,
        mint: String,
        wallet: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            run_id,
            strategy_id,
            rule_id: Some(rule_id),
            mode,
            mint_address: mint,
            wallet,
            token_program_id: None,
            token_account: None,
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: None,
            entry_token_amount: None,
            entry_sol: None,
            entry_time: None,
            entry_tx_signatures: json!([]),
            exit_price: None,
            exit_token_amount: None,
            exit_sol: None,
            exit_time: None,
            exit_tx_signatures: json!([]),
            sold_token_amount: 0,
            exit_sol_total: 0.0,
            scale_stage: 0,
            submitted_buy_signatures: Vec::new(),
            status: "BuySubmitted".to_string(),
            exit_reason: None,
            origin: "bot".to_string(),
            manual_exit: None,
            exit_redrive_count: 0,
            exit_parked: false,
            last_entry_error: None,
            needs_review: false,
            extra: json!({}),
            created_at: now,
            updated_at: now,
        }
    }

    /// Tokens still held: `entry - sold` (scale-out remainder).
    pub fn remaining_token_amount(&self) -> u64 {
        self.entry_token_amount
            .unwrap_or(0)
            .saturating_sub(self.sold_token_amount)
    }

    /// Sold fraction of the initial bag in bps (`sold * 10_000 / entry`).
    pub fn sold_bps(&self) -> u16 {
        bps_of_bag(self.sold_token_amount, self.entry_token_amount)
    }
}

/// Fraction of the **initial** bag one raw-token quantity represents, in bps.
/// The ONE definition: the position-level `sold_bps` rollup and every per-leg
/// `ExitFillLeg::sell_bps` read through it, so a leg and the aggregate can never
/// scale a share differently. Widened to `u128` because a raw pump.fun bag times
/// 10_000 overflows `u64`; a zero/absent entry bag yields 0 (nothing to divide).
pub fn bps_of_bag(part: u64, entry: Option<u64>) -> u16 {
    let entry = entry.unwrap_or(0);
    if entry == 0 {
        return 0;
    }
    ((u128::from(part) * 10_000) / u128::from(entry)).min(10_000) as u16
}

/// One exit fill on the wire, for chart markers: a scale-out ladder draws one
/// arrow per leg instead of a single arrow at the SOL-weighted average price —
/// a point that never traded.
///
/// SSOT: **the** exit-leg wire shape. Simulate rows (`lab`'s engine results) and
/// traded positions (`PositionResponse`) both serialize this, so the frontend has
/// one `exit_legs` contract and one marker builder for a modeled and a real
/// ladder alike.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitFillLeg {
    /// Share of the initial bag this leg sold ([`bps_of_bag`]).
    pub sell_bps: u16,
    pub price: f64,
    pub time: DateTime<Utc>,
    pub tx: Option<String>,
    /// Exit reason that fired this leg; `None` on a legacy row with no per-leg reason.
    pub reason: Option<String>,
}

/// One append-only fill leg under a `strategy_positions` episode (mig 0018
/// `position_fills`). Entry is `side = buy`; each confirmed sell (partial or
/// final) is `side = sell`. Per-leg PnL% / hold time are derived at read time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionFill {
    pub position_id: Uuid,
    pub seq: i32,
    /// `"buy"` | `"sell"`.
    pub side: String,
    pub price: f64,
    /// Exact SOL amount for this leg (lamports).
    pub sol_lamports: i64,
    /// Raw token units for this leg.
    pub token_amount: u64,
    pub at: DateTime<Utc>,
    pub reason: Option<String>,
    /// Scale-out stage index that produced this sell (`None` on buy / legacy).
    pub stage: Option<i16>,
    pub tx_signature: Option<String>,
}

impl StrategyPosition {
    /// Record the target (trigger-trade) snapshot that armed this position.
    pub fn set_target(&mut self, price: f64, amount: u64, time: DateTime<Utc>, tx: String) {
        self.target_price = Some(price);
        self.target_token_amount = Some(amount);
        self.target_time = Some(time);
        self.target_tx = Some(tx);
        self.updated_at = Utc::now();
    }

    /// Append a submitted snipe-buy signature and flip to `BuySubmitted` (the
    /// durable "buy in flight" marker; every attempt is recoverable).
    pub fn mark_buy_submitted(&mut self, signature: String) {
        self.submitted_buy_signatures.push(signature);
        self.status = "BuySubmitted".to_string();
        self.updated_at = Utc::now();
    }

    /// Record the entry fill + flip to `Holding`.
    pub fn set_entry(
        &mut self,
        price: f64,
        token_amount: u64,
        sol: f64,
        time: DateTime<Utc>,
        tx_signatures: Vec<String>,
    ) {
        self.entry_price = Some(price);
        self.entry_token_amount = Some(token_amount);
        self.entry_sol = Some(sol);
        self.entry_time = Some(time);
        self.entry_tx_signatures = json!(tx_signatures);
        self.status = "Holding".to_string();
        self.updated_at = Utc::now();
    }

    /// Flip to `Holding` (fill adopted/stamped elsewhere).
    pub fn mark_entry_filled(&mut self) {
        self.status = "Holding".to_string();
        self.updated_at = Utc::now();
    }

    /// Flip to `ExitPending` while the sell is in flight.
    pub fn mark_exit_pending(&mut self) {
        self.status = "ExitPending".to_string();
        self.updated_at = Utc::now();
    }

    /// The buy never filled (entry exhausted / fatal) — terminal, no SOL deployed.
    /// Deliberately stamps NO exit price/time: there was never a position, and a
    /// hypothetical exit would pollute PnL surfaces (the row is excluded from
    /// realized PnL by its NULL entry).
    pub fn mark_entry_failed(&mut self) {
        self.status = "EntryFailed".to_string();
        self.updated_at = Utc::now();
    }

    /// The sell gave up and the bag is still held — the position stays OPEN as
    /// `ExitStuck` (attention lane). No exit price is stamped (nothing sold; the
    /// row keeps marking to market). The reaper re-drives it, then parks it for a
    /// manual Retry / Dump / Write-off.
    pub fn mark_exit_stuck(&mut self) {
        self.status = "ExitStuck".to_string();
        self.updated_at = Utc::now();
    }

    /// Mark the exit **unconfirmed** (C1): the sell may have landed — or may still
    /// land — but the trade feed never confirmed the clear and the tx did **not**
    /// revert, so re-selling automatically would risk a double-sell. The engine
    /// NEVER auto-re-sells it; the row stays OPEN (attention lane) with manual
    /// Verify / Re-sell / Write-off, plus the reaper's bag-cleared heal. No
    /// hypothetical exit price is stamped.
    pub fn mark_exit_unconfirmed(&mut self) {
        self.status = "ExitUnconfirmed".to_string();
        self.updated_at = Utc::now();
    }

    /// Close with a confirmed exit fill (`End`).
    #[allow(clippy::too_many_arguments)]
    pub fn close(
        &mut self,
        exit_price: f64,
        exit_sol: f64,
        exit_token_amount: u64,
        exit_tx_signatures: Vec<String>,
        exit_time: DateTime<Utc>,
        reason: &str,
    ) {
        self.exit_price = Some(exit_price);
        self.exit_sol = Some(exit_sol);
        self.exit_token_amount = Some(exit_token_amount);
        self.exit_tx_signatures = json!(exit_tx_signatures);
        self.exit_time = Some(exit_time);
        self.exit_reason = Some(reason.to_string());
        self.status = "End".to_string();
        self.updated_at = Utc::now();
    }

    /// Record sells that were **submitted** but never confirmed as a fill, without
    /// touching `exit_price`/`exit_time`/`status` (this is not a close).
    ///
    /// Unions into the existing array — a reaper redrive re-enters the same sink
    /// path, and overwriting would erase the signatures an earlier attempt left
    /// behind. Those signatures are the only durable evidence distinguishing "the
    /// sell landed and the feed missed it" from "the sell never landed", which is
    /// what `ExitUnconfirmed`/`ExitStuck` triage turns on.
    pub fn add_submitted_exit_sigs(&mut self, sigs: impl IntoIterator<Item = String>) {
        let mut have = json_str_array(&self.exit_tx_signatures);
        let mut added = false;
        for s in sigs {
            if !have.contains(&s) {
                have.push(s);
                added = true;
            }
        }
        if added {
            self.exit_tx_signatures = json!(have);
            self.updated_at = Utc::now();
        }
    }

    /// Entry fill signatures (the JSONB array decoded to a `Vec`).
    pub fn entry_tx_sigs(&self) -> Vec<String> {
        json_str_array(&self.entry_tx_signatures)
    }

    /// Exit fill signatures (the JSONB array decoded to a `Vec`).
    pub fn exit_tx_sigs(&self) -> Vec<String> {
        json_str_array(&self.exit_tx_signatures)
    }
}

fn default_origin() -> String {
    "bot".to_string()
}

/// Decode a JSON string-array `Value` into a `Vec<String>` (non-strings skipped).
fn json_str_array(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod submitted_exit_sig_tests {
    use super::*;

    fn row() -> StrategyPosition {
        StrategyPosition::new(
            Uuid::new_v4(),
            "generic".to_string(),
            Uuid::new_v4(),
            "real".to_string(),
            "MINT".to_string(),
            "WALLET".to_string(),
        )
    }

    /// A sell that was submitted but never confirmed must still leave a trace.
    /// Regression (2026-07-28, mint 57aJ…): the executor only persisted sell
    /// signatures on the SUCCESS path, so a row could sit in `ExitUnconfirmed`
    /// with an empty array despite having sent sells — destroying the only
    /// evidence that separates "landed late" from "never landed".
    #[test]
    fn records_submitted_sells_without_closing_the_position() {
        let mut p = row();
        p.add_submitted_exit_sigs(["sigA".to_string()]);
        assert_eq!(p.exit_tx_sigs(), vec!["sigA".to_string()]);
        // Not a close: no fill fields, no status change.
        assert!(p.exit_price.is_none());
        assert!(p.exit_time.is_none());
    }

    /// A reaper redrive re-enters the same sink path; recording must union, or
    /// each retry would erase the previous attempt's signatures.
    #[test]
    fn unions_across_redrives_and_ignores_duplicates() {
        let mut p = row();
        p.add_submitted_exit_sigs(["sigA".to_string(), "sigB".to_string()]);
        p.add_submitted_exit_sigs(["sigB".to_string(), "sigC".to_string()]);
        assert_eq!(
            p.exit_tx_sigs(),
            vec!["sigA".to_string(), "sigB".to_string(), "sigC".to_string()]
        );
    }
}

#[cfg(test)]
mod bps_of_bag_tests {
    use super::*;

    #[test]
    fn scales_a_leg_against_the_initial_bag() {
        assert_eq!(bps_of_bag(700, Some(1_000)), 7_000);
        assert_eq!(bps_of_bag(1_000, Some(1_000)), 10_000);
    }

    #[test]
    fn a_raw_pumpfun_bag_does_not_overflow() {
        // 1e9 tokens at 6 decimals = 1e15 raw units; times 10_000 that is 1e19,
        // past u64::MAX — the reason this widens to u128 rather than doing the
        // multiply in i64/SQL.
        let bag = 1_000_000_000_u64 * 1_000_000;
        assert_eq!(bps_of_bag(bag / 2, Some(bag)), 5_000);
        assert_eq!(bps_of_bag(bag, Some(bag)), 10_000);
    }

    #[test]
    fn clamps_a_leg_that_outgrew_its_entry_bag() {
        // Airdrop / re-buy into the same account can sell more than was entered;
        // a share over 100% would render as a nonsense marker label.
        assert_eq!(bps_of_bag(2_000, Some(1_000)), 10_000);
    }

    #[test]
    fn no_entry_bag_is_zero_not_a_divide() {
        assert_eq!(bps_of_bag(500, None), 0);
        assert_eq!(bps_of_bag(500, Some(0)), 0);
    }

    #[test]
    fn sold_bps_reads_through_the_one_formula() {
        let mut p = StrategyPosition::new(
            Uuid::new_v4(),
            "generic".to_string(),
            Uuid::new_v4(),
            "real".to_string(),
            "MINT".to_string(),
            "WALLET".to_string(),
        );
        p.entry_token_amount = Some(1_000);
        p.sold_token_amount = 250;
        assert_eq!(p.sold_bps(), bps_of_bag(250, Some(1_000)));
    }
}

#[cfg(test)]
mod pnl_pct_tests {
    use super::*;

    /// A closed position: `entry_sol` in, `exit_sol` out, at the given prices.
    fn closed(entry_sol: f64, exit_sol: f64, entry_price: f64, exit_price: f64) -> StrategyPosition {
        let mut p = StrategyPosition::new(
            Uuid::new_v4(),
            "generic".to_string(),
            Uuid::new_v4(),
            "real".to_string(),
            "MINT".to_string(),
            "WALLET".to_string(),
        );
        p.status = "End".to_string();
        p.entry_sol = Some(entry_sol);
        p.entry_price = Some(entry_price);
        p.exit_sol = Some(exit_sol);
        p.exit_price = Some(exit_price);
        p
    }

    /// The headline defect this formula was changed to fix: the price moved up,
    /// but the round trip lost SOL after fees. A price ratio reports +2% (green)
    /// next to a negative SOL figure (red); a capital return cannot.
    #[test]
    fn a_price_gain_that_lost_money_reports_a_loss() {
        // Price +2%, but only 0.098 SOL came back out of 0.1 in — the spread the
        // venue fee + tip + impact ate.
        let p = closed(0.1, 0.098, 1.0, 1.02);
        let pct = p.pnl_pct().expect("closed position has a percent");
        assert!(pct < 0.0, "expected a loss, got {pct}%");
        assert!((pct - (-2.0)).abs() < 1e-9, "expected -2%, got {pct}%");
        assert!(p.realized_pnl_sol().unwrap() < 0.0);
    }

    /// Sign-lock: the percent and the SOL figure are the same number over a
    /// positive denominator, so they can never point opposite ways.
    #[test]
    fn sign_is_locked_to_realized_sol() {
        for (entry, exit) in [(0.1, 0.15), (0.1, 0.05), (0.1, 0.1)] {
            let p = closed(entry, exit, 1.0, 1.0);
            let sol = p.realized_pnl_sol().unwrap();
            let pct = p.pnl_pct().unwrap();
            assert_eq!(
                sol.partial_cmp(&0.0),
                pct.partial_cmp(&0.0),
                "sol {sol} and pct {pct} disagree"
            );
        }
    }

    /// A laddered exit books every leg. The old formula read `exit_price` — the
    /// LAST leg only — so a position that sold most of its bag high and the tail
    /// low reported the tail's price as the whole trade's outcome.
    #[test]
    fn scale_out_percent_reads_every_leg_not_the_last_price() {
        let mut p = closed(0.1, 0.0, 1.0, 0.5); // last leg dumped at half price
        p.sold_token_amount = 1_000;
        p.exit_sol_total = 0.13; // 0.13 SOL actually came back across all legs
        let pct = p.pnl_pct().expect("closed position has a percent");
        assert!((pct - 30.0).abs() < 1e-9, "expected +30% on capital, got {pct}%");
    }

    /// No capital deployed ⇒ no return to report (never a fabricated 0%).
    #[test]
    fn unentered_and_open_rows_have_no_percent() {
        let mut p = closed(0.1, 0.2, 1.0, 2.0);
        p.entry_sol = None;
        assert!(p.pnl_pct().is_none(), "an unentered row has no capital base");

        let mut open = closed(0.1, 0.2, 1.0, 2.0);
        open.exit_sol = None;
        open.exit_sol_total = 0.0;
        assert!(open.pnl_pct().is_none(), "an open row has no realized exit");
    }
}
