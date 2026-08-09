use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::trade::TradeType;

/// Lightweight signal for the strategy hot path (reads `TokenCache` after update).
#[derive(Debug, Clone)]
pub struct StrategyPing {
    pub mint: String,
    pub kind: IngestKind,
    /// Transport observation time for create pings (`TokenCreated.received_at`).
    /// `None` on trade/migrate/creator-activity lanes — only the create fast
    /// lane carries an end-to-end latency stamp (L0).
    pub received_at: Option<DateTime<Utc>>,
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
        /// Per-TRANSACTION network fee in SOL (see `Trade::fee_sol`). Carried on the
        /// live lane so an SSE-appended row shows the same fee a refetch would —
        /// without it the newest rows in the trade table would read "—" until the
        /// next full fetch. `None` when the feed carried no fee.
        fee_sol: Option<f64>,
        tx_signature: String,
        /// Intra-slot order keys — same canonical `(slot, tx_index, leg_index)` the
        /// REST trade history and chart aggregators use. Required so live SSE
        /// appends sort identically to a full refetch (no bar reordering drift).
        tx_index: u32,
        leg_index: u32,
        /// Venue-neutral post-trade reserves (curve virtual / AMM real) so the
        /// live chart tip can use reserve spot, not only `price_per_token`.
        reserve_sol: Option<f64>,
        reserve_token: Option<f64>,
        /// `"curve"` | `"amm"` — matches `trades.venue` / `TradeRecord.venue`.
        venue: String,
        /// Ordered ix-structure labels for this trade's tx (see
        /// `Trade::instruction_labels`). Carried on the live lane because the chart
        /// classifies vol / non-vol from these labels client-side: an appended row
        /// without them is silently counted as non-vol AND never tags its wallet,
        /// so the whole cumulative pair diverges from that point until a refetch.
        /// Empty when the decoder captured none.
        instruction_labels: Vec<String>,
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
    /// Progress of an in-flight metric-combo discovery pipeline (lab only). `phase`
    /// is `"corpus"` (lake load; `total: 0` ⇒ indeterminate), `"screen"` (Layer 1),
    /// `"family"` (Layer 2), or `"validate"` (Layer 3) — the pipeline re-declares the
    /// total at each layer, so `processed`/`total` reset per phase. Not mint-scoped —
    /// always delivered.
    MetricDiscoveryProgress {
        run_id: uuid::Uuid,
        phase: String,
        processed: u64,
        total: u64,
    },
    /// Terminal frame for the metric-combo discovery pipeline (`cancelled` / optional
    /// `error`). The client's only reliable signal the run ended.
    MetricDiscoveryFinished {
        run_id: uuid::Uuid,
        cancelled: bool,
        error: Option<String>,
    },
    /// Non-fatal notice during the pipeline (e.g. token_cap truncation, a degenerate
    /// validation split).
    MetricDiscoveryNotice {
        run_id: uuid::Uuid,
        message: String,
    },
    /// A generic-engine position transition, emitted by the engine's
    /// `PositionUpdate` sink. Mint-scoped. `status` is the `strategy_positions`
    /// lifecycle string (`BuySubmitted` | `Holding` | `ExitPending` | `End` |
    /// `EntryFailed` | `ExitStuck` | `ExitUnconfirmed`); `exit_reason` is set on
    /// exit statuses. `trade_mode` / `rule_name` let notification toasts filter
    /// real vs paper without a round-trip. The client patches the one position
    /// row in place.
    StrategyPositionUpdate {
        rule_id: uuid::Uuid,
        mint_address: String,
        position_id: uuid::Uuid,
        status: String,
        exit_reason: Option<String>,
        entry_price: Option<f64>,
        /// Entry-fill economics, carried so a session hydrated purely from deltas
        /// can draw the chart's entry marker and print the entry size. The REST
        /// snapshot is the only other source, and it refetches on mount / SSE
        /// reopen / tab-visible — a position that opens mid-session would otherwise
        /// have no entry time at all until the next one.
        entry_sol: Option<f64>,
        entry_time: Option<chrono::DateTime<chrono::Utc>>,
        exit_price: Option<f64>,
        /// `"real"` | `"paper"` from the frozen position meta (fallback: rule table).
        trade_mode: Option<String>,
        rule_name: Option<String>,
        /// `Some(true)` on a stale, unresolved `BuySubmitted` past the review
        /// window (B3): the reaper could neither adopt a fill nor prove every sig
        /// reverted — the row needs a manual Verify. `None` elsewhere.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        needs_review: Option<bool>,
        /// Confirmed sell-leg raw token units so far (scale-out). `None` when zero
        /// / legacy. FE chip: banked fraction of the initial bag.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        sold_token_amount: Option<u64>,
        /// Sold fraction of the initial bag in bps. `None` when zero / legacy.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        sold_bps: Option<u16>,
        /// Next scale-out stage index after this update. `None` when unset / legacy.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        scale_stage: Option<u8>,
    },
    /// A generic-engine (token, rule) arming transition — armed or disarmed —
    /// emitted by the engine's `ArmedChanged` sink. There is no legacy analogue
    /// (armed state was pull-only before the redesign). Mint-scoped. `state` is
    /// `"armed"` | `"disarmed"`; `reason` is the disarm reason (`dead` | `migrated`
    /// | `unsatisfiable` | `entered`) when disarmed. `entered` means buy left
    /// Waiting (not a toastable disarm). `trade_mode` / `rule_name` mirror
    /// [`StrategyPositionUpdate`] so notification toasts can filter real vs paper
    /// without a round-trip.
    StrategyArmedChanged {
        rule_id: uuid::Uuid,
        mint_address: String,
        state: String,
        reason: Option<String>,
        /// `"real"` | `"paper"` when the sink still has the rule loaded.
        trade_mode: Option<String>,
        rule_name: Option<String>,
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
    /// Broadcast when the SSE render bridge drops frames (`Lagged`). Not
    /// mint-scoped — clients must refetch Live Status / other delta views.
    SseResync,
}
