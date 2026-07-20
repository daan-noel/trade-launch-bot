use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::trade::TradeType;

/// Lightweight signal for the strategy hot path (reads `TokenCache` after update).
#[derive(Debug, Clone)]
pub struct StrategyPing {
    pub mint: String,
    pub kind: IngestKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestKind {
    TokenCreated,
    Trade,
    Migrated,
    CreatorActivity,
}

/// Cold-lane SSE notification (enriched from cache in the HTTP handler).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    TokenCreated {
        mint_address: String,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    TradeExecuted {
        mint_address: String,
        wallet: String,
        trade_type: TradeType,
        amount_sol: f64,
        /// Raw token units — exact `u64` (serializes as a JSON number; the frontend
        /// scales to a display amount). No `f64` round-trip on the notify path.
        token_amount: u64,
        price_per_token: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    LiquidityAdded {
        mint_address: String,
        wallet: String,
        amount_sol: f64,
        token_amount: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    LiquidityRemoved {
        mint_address: String,
        wallet: String,
        amount_sol: f64,
        token_amount: f64,
        tx_signature: String,
        slot: u64,
        timestamp: DateTime<Utc>,
    },
    /// A paper-test run reached its total-token cap and every position exited;
    /// the rule was auto-deactivated. Not mint-scoped — always delivered.
    PaperTestFinished {
        rule_id: uuid::Uuid,
        rule_name: String,
        run_seq: i64,
        tokens_traded: i64,
        timestamp: DateTime<Utc>,
    },
    /// A tpsl rule list changed for `strategy` ("tpsl1" | "tpsl2") — a rule was
    /// created, updated, deleted, or moved through a lifecycle transition. A bare
    /// signal (no payload beyond the strategy); the client refetches the list.
    /// Not mint-scoped — always delivered.
    TpslRulesChanged { strategy: String },
    /// Progress of an in-flight simulation (backtest) for `rule_id`: `processed`
    /// of `total` candidate tokens resolved. Lets the dashboard show real
    /// percentages instead of a fake trickle bar. Throttled to ~100 frames per
    /// run plus a final `processed == total`. Not mint-scoped — always delivered.
    SimulationProgress {
        rule_id: uuid::Uuid,
        processed: u64,
        total: u64,
    },
    /// Progress of an in-flight grouped param-sweep for `strategy_id`: `processed`
    /// of `total` candidate tokens folded across all surviving groups. `phase`
    /// identifies which phase is reporting (`"corpus"` = lake load, `total: 0` ⇒
    /// indeterminate; `"coarse"` = refine runs only; `"sweep"` = the folding
    /// pass). Persistence is NOT a phase — it reports via [`SseEvent::SweepGroupDone`].
    /// Throttled to ~100 frames per run plus a final `processed == total`
    /// (or the count where it was cancelled). Not mint-scoped — always delivered.
    /// No rate/ETA rides this frame **by design**: the client derives both from
    /// `processed`/`total` over wall-clock (`estimateEtaMs` in the jobs indicator),
    /// one implementation shared by sweeps, simulations and swings. A second,
    /// server-side ETA would be the same fact computed twice — free to drift.
    SweepProgress {
        strategy_id: String,
        phase: String,
        processed: u64,
        total: u64,
    },
    /// One grouped-sweep group has been fully folded **and persisted** — the
    /// signal that its rows are readable via the run's `groups` endpoint right
    /// now, mid-run. Emitted by the sweep's DB-writer task once per committed
    /// group, plus one announce frame (`group_index: None`, `groups_done: 0`)
    /// when the surviving group/combo counts are first known. Lets the frontend
    /// stream group results into the table while the sweep is still folding,
    /// and drive a "persisted N/M" counter. Not mint-scoped — always delivered.
    SweepGroupDone {
        strategy_id: String,
        run_id: uuid::Uuid,
        /// Deterministic index of the group that just persisted; `None` on the
        /// initial announce frame.
        group_index: Option<u64>,
        groups_done: u64,
        group_count: u64,
    },
    /// Terminal frame for the grouped sweep: the single-flight run for
    /// `strategy_id` has ended (`cancelled` distinguishes a user abort from a
    /// normal finish/error). Lets a global progress indicator clear itself
    /// without polling. Not mint-scoped — always delivered. `error` carries the
    /// reason when the run failed *after* admission (e.g. a RAM-admission
    /// refusal) — that path deletes the run row, so this frame is the client's
    /// only signal, and it surfaces the string as a toast instead of a frozen
    /// page. `None` on a normal finish or a user cancel.
    SweepFinished {
        strategy_id: String,
        cancelled: bool,
        error: Option<String>,
    },
    /// Non-fatal operational notice for a running grouped sweep — today, that the
    /// engine degraded its sizing to fit free RAM (fewer threads, smaller fold
    /// buffers). The run still completes and its results are unaffected; it is just
    /// slower, and saying so is what keeps a degraded run from reading as a stall.
    /// Emitted a handful of times at sweep start, never from the fold loop. Not
    /// mint-scoped — always delivered.
    SweepNotice {
        strategy_id: String,
        message: String,
    },
    /// Terminal frame for a rule simulation (backtest): the run for `rule_id` has
    /// ended. The per-rule analogue of [`SseEvent::SweepFinished`]. Not
    /// mint-scoped — always delivered.
    SimulationFinished {
        rule_id: uuid::Uuid,
        cancelled: bool,
    },
    /// Progress of an in-flight flow-discovery job. `phase` is `"corpus"` (lake
    /// load; `total: 0` ⇒ indeterminate) or `"score"` (per-token fold). Not
    /// mint-scoped — always delivered.
    FlowDiscoveryProgress {
        run_id: uuid::Uuid,
        phase: String,
        processed: u64,
        total: u64,
    },
    /// Terminal frame for flow discovery (`cancelled` / optional `error`).
    FlowDiscoveryFinished {
        run_id: uuid::Uuid,
        cancelled: bool,
        error: Option<String>,
    },
    /// Non-fatal notice during discovery (e.g. token_cap truncation).
    FlowDiscoveryNotice {
        run_id: uuid::Uuid,
        message: String,
    },
    /// A generic-engine position transition, emitted by the engine's
    /// `PositionUpdate` sink. Mint-scoped. `status` is the `strategy_positions`
    /// lifecycle string (`BuySubmitted` | `Holding` | `ExitPending` | `End` |
    /// `ExitFailed` | `ExitUnconfirmed`); `exit_reason` is set on exit statuses.
    /// `trade_mode` / `rule_name` let notification toasts filter real vs paper
    /// without a round-trip. The client patches the one position row in place.
    StrategyPositionUpdate {
        rule_id: uuid::Uuid,
        mint_address: String,
        position_id: uuid::Uuid,
        status: String,
        exit_reason: Option<String>,
        entry_price: Option<f64>,
        exit_price: Option<f64>,
        /// `"real"` | `"paper"` when the sink still has the rule loaded.
        trade_mode: Option<String>,
        rule_name: Option<String>,
    },
    /// A generic-engine (token, rule) arming transition — armed or disarmed —
    /// emitted by the engine's `ArmedChanged` sink. There is no legacy analogue
    /// (armed state was pull-only before the redesign). Mint-scoped. `state` is
    /// `"armed"` | `"disarmed"`; `reason` is the disarm reason (`dead` | `migrated`
    /// | `unsatisfiable`) when disarmed.
    StrategyArmedChanged {
        rule_id: uuid::Uuid,
        mint_address: String,
        state: String,
        reason: Option<String>,
    },
    /// Progress of a long-running operator action (Stop & close / Stop All).
    /// Emitted at start (`running`, `done = 0`), as each position reaches a
    /// terminal exit status, and once at the terminal outcome (`done` |
    /// `partial` | `failed`). Same field vocabulary as forge's `action_progress`
    /// so the frontend hooks stay near-identical. Not mint-scoped — always
    /// delivered (`mint_address` is `None` for rule-scoped stops).
    ActionProgress {
        action_id: uuid::Uuid,
        mint_address: Option<String>,
        rule_id: Option<uuid::Uuid>,
        /// `"stop"` | `"sell"` — drives the label tone.
        kind: String,
        /// `running` | `partial` | `done` | `failed`.
        status: String,
        done: u64,
        total: u64,
        error: Option<String>,
    },
}
