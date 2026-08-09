//! Grouped param-sweep persistence models — strategy-agnostic.
//!
//! One sweep run partitions its corpus into fingerprint groups and ranks param
//! combos within each group. These map the per-strategy tables
//! (`<strategy>_grouped_sweep_runs` / `_groups` / `_results`) the registry
//! resolves; the generic repo is table-name-driven, so a new strategy reuses
//! these models verbatim. Serialize-only — the API never deserializes them from
//! the client; field names are the JSON the frontend tables bind to.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// One grouped-sweep invocation header: which strategy over what selection, the
/// grouping fields, the resolved axes, and the realised population counts.
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepRun {
    pub id: Uuid,
    pub strategy_id: String,
    pub source: String,
    pub method: String,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub curve_only: bool,
    /// The grouping fields, e.g. `["creator_wallet","max_cost_lamports"]`.
    pub grouping_spec: Value,
    /// The resolved param axes (post-defaults/dedup) for echo / re-run.
    pub axes_spec: Value,
    pub min_tokens: i32,
    pub token_count: i32,
    pub group_count: i32,
    pub combo_count: i32,
    pub corpus_hash: Option<String>,
    /// Block time of the **newest trade in the corpus this run scanned** — how fresh
    /// its data was. `None` on legacy rows / a trade-less corpus.
    ///
    /// The sweep reads the sealed Parquet lake only, while `simulate` splices the fresh
    /// PG tail on top, so a stale lake export makes the two disagree without either
    /// being wrong: the sweep freezes positions as `Open (est)` at old prices that a
    /// simulate watches die. This is the number that makes that visible ("data through
    /// HH:MM") — and it is the same instant the frozen-tail resolve anchors on.
    pub corpus_last_trade_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// The exact-set instruction-label corpus filter the run used, as the JSON
    /// array the request sent (`None` = no filter / grouped by `ix_labels`).
    /// Persisted so the history panel can show what the sweep was pinned to and
    /// the re-run can restore it.
    pub ix_labels_filter: Option<Value>,
    /// Per-field value filters the corpus was pinned to (`{"cu_price":[1000],…}`);
    /// `None` = no field filter. Stored verbatim for display + re-run.
    pub field_filters: Option<Value>,
    /// Saved fingerprint the corpus was scoped to (engine match SSOT — exact +
    /// bucket axes), or `None` for a manual group-by / filter run. Mutually
    /// exclusive with `ix_labels_filter` / `field_filters`: when set, the run
    /// matched tokens with `hunter_engine::fingerprint::matches` instead, so the
    /// scope cannot be reconstructed from those columns and must be replayed
    /// from this id (re-run + token-results reload).
    pub fingerprint_id: Option<Uuid>,
    /// The per-run token cap the form submitted (`None` = legacy / backend
    /// default). Distinct from the realized `token_count`.
    pub token_cap: Option<i32>,
    /// The per-group combo-cap override the form submitted (`None` = backend
    /// default). Distinct from the realized `combo_count`.
    pub max_combos: Option<i32>,
    /// Optional user-given name for the run (`None` = unnamed → UI falls back to
    /// timestamp + grouping hint). Editable via the rename endpoint.
    pub label: Option<String>,
    /// Notional (SOL) every simulated round-trip in this run was priced at.
    /// `None` on legacy rows — callers fall back to the server default (1.0 SOL).
    pub buy_amount_sol: Option<f64>,
    /// Per-run bucket width (SOL) the continuous SOL grouping fields were binned at
    /// — the same width the created rule's matcher + the creation-stats dashboard
    /// use ("swept = run"). `None` on legacy rows → callers fall back to the default
    /// (`grouping::SOL_BUCKET_WIDTH`, 0.1). Stored for display + re-run + promotion.
    pub bucket_width_sol: Option<f64>,
    /// Which trade in the fill window priced each leg
    /// (`worst_case` | `first_in_window` | `signal_price`). `None` on legacy rows ⇒
    /// `worst_case`, what the sweep hardcoded before the model became selectable.
    /// Part of the run's **identity**: two runs under different fill models are not
    /// comparable, so the UI must show it next to the PnL.
    pub fill_model: Option<String>,
    /// Which execution-cost model priced the round-trips (`pumpfun_default` |
    /// `pumpfun_fee_only`). `None` on legacy rows ⇒ `pumpfun_default`. Pairing
    /// `pumpfun_default` with an explicit fill model double-counts slippage — see
    /// `CostModelKind`.
    pub cost_model: Option<String>,
    /// Optional volume-ix pattern set for flow-metric sweeps (`string[][]`).
    /// Compiled corpus-wide into `FlowPatterns`; Promote copies into the
    /// fingerprint's `metric_config.m_flow_split.volume_ix_patterns`. `None` =
    /// non-flow run / legacy row.
    pub volume_ix_patterns: Option<Value>,
    /// The candidate scale-out ladder **grid** searched in Pass 2 (`ExitStage[][]`
    /// — one array per candidate ladder). `None` = no Pass 2 (legacy / overlay
    /// off). This is the config that was searched, NOT necessarily what any one
    /// combo ended up with: each group's top-K combos are independently re-scored
    /// against every ladder here plus their own baseline and keep whichever wins,
    /// so the winning ladder (if any) is baked directly into that specific
    /// combo's own `_combos.params` / `best_params` at write time — never merged
    /// from this run-level field at read time. See
    /// `docs/arch/sweep.md` (*Pass-2 overlay*).
    pub scale_out: Option<Value>,
    /// How many best combos per group Pass 2 re-scores. `None` when no overlay.
    pub scale_out_top_k: Option<i32>,
    /// Lifecycle: `running` (in flight), `completed` (full sweep), or
    /// `cancelled` (cancelled / crash-recovered → only `groups_done` groups
    /// present). With incremental persistence a `cancelled` run is honest about
    /// being partial so the UI never shows it as a complete sweep.
    pub status: String,
    /// Groups persisted so far; equals `group_count` for a `completed` run, fewer
    /// for a `cancelled`/partial one. Drives the run picker's "37 / 200 groups".
    pub groups_done: i32,
}

/// One group's summary row (the group-list table): its fingerprint key, sample
/// size, and the winning combo. The winner is picked on the robust realized
/// `best_score` (the headline metric); `fired_count` is its `n_fired` — the
/// sample size behind the pick — and `best_expectancy_sol` its expectancy
/// (a secondary readout, not the ranking metric).
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepGroupSummary {
    pub id: Uuid,
    pub group_index: i32,
    pub group_key: Value,
    pub token_count: i32,
    pub fired_count: i64,
    pub best_combo_id: i32,
    /// Checklist `score` of the winning combo; `None` when it never fired.
    pub best_score: Option<f64>,
    pub best_expectancy_sol: f64,
    // --- Winning combo's full stat line (JOINed from the `_results` row for
    // `best_combo_id`, which retention always keeps — see `retained_combo_ids`).
    // These are the same numbers the drill-in "Combos for group" table shows for
    // the crowned combo, surfaced on the group row so the headline is a full
    // readout, not just score/expectancy. SSOT: read from `_results`, never
    // re-persisted onto the group row. ---
    /// Winning combo's win rate (fraction 0..1).
    pub best_win_rate: f64,
    /// Winning combo's realized total PnL in SOL — the group table's default sort.
    pub best_total_pnl_sol: f64,
    /// Winning combo's **unrealized** PnL: the mark-to-last-price sum over its
    /// still-`Open` positions, excluded from `best_total_pnl_sol` by design.
    /// Surfaced so a group whose realized total looks profitable can't hide a
    /// pile of open losers — `best_total_pnl_sol + best_open_pnl_sol` is the
    /// mark-to-market readout.
    pub best_open_pnl_sol: f64,
    /// Winning combo's still-open / closed position counts. `fired_count` above
    /// is the total (`best_n_open + best_n_closed`); these split it so the group
    /// table can show how much of the sample is unrealized — a headline built on
    /// 3 closed and 40 open trades is a different claim than one on 43 closed.
    pub best_n_open: i64,
    pub best_n_closed: i64,
    /// Winning combo's profit factor; `None` = no losing trades (UI shows ∞).
    pub best_profit_factor: Option<f64>,
    pub best_mean_pnl_pct: f64,
    pub best_median_pnl_pct: f64,
    pub best_p90_pnl_pct: f64,
    pub best_std_pnl_pct: f64,
    pub best_avg_holding_secs: f64,
    pub best_median_holding_secs: f64,
    pub best_params: Value,
    /// What this group's tokens were actually selected by — the scope
    /// fingerprint's axes, the run's manual filters and the group key, resolved
    /// into one canonical clause list by `lab`'s `sweep::selection::GroupSelection`
    /// (which also says whether it can be promoted, and why not).
    ///
    /// Not a stored column: every input lives on the run row, so this is derived
    /// per request by the ONE resolver rather than persisted as a second copy that
    /// could drift from it. `None` on paths that don't need it (the repo leaves it
    /// unset; only the groups-list handler fills it in).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Value>,
}

/// One ranked param-combo row within a group (the drill-in table). Metric set
/// matches the flat per-combo `ComboMetrics` so the frontend reuses
/// `buildSweepColumns`.
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepResult {
    pub combo_id: i32,
    pub params: Value,
    pub n_fired: i64,
    pub n_open: i64,
    pub n_closed: i64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    /// Unrealized mark-to-last-price sum over this combo's still-`Open`
    /// positions. Never included in `total_pnl_sol` (realized-only).
    pub open_pnl_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    /// Stddev of realized per-trade pnl% (display).
    pub std_pnl_pct: f64,
    /// `None` = no losing trades (infinite profit factor); UI shows ∞.
    pub profit_factor: Option<f64>,
    /// Checklist rank; `None` when nothing fired.
    pub score: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    /// Per-exit-reason trade counts — how many of this combo's closed trades
    /// terminated on each reason. Counts, **not** params: distinct from the
    /// `exit_take_profit`/`exit_stop_loss` *threshold* knobs inside `params`.
    pub n_exit_take_profit: i32,
    pub n_exit_stop_loss: i32,
    pub n_exit_trailing: i32,
    pub n_exit_stall: i32,
    pub n_exit_time: i32,
    pub n_exit_liquidity: i32,
    /// Analysis-only death-closes: positions closed at the last meaningful trade
    /// because the token died silent (see `trading_core::strategies::death`).
    pub n_exit_dead: i32,
    /// Generic-engine metric-condition exits (`ExitCode::Metrics`); 0 for the
    /// legacy tpsl/swing sweeps.
    pub n_exit_metrics: i32,
    /// `n_exit_metrics` broken down by which of the rule's own authored exit
    /// conditions fired (0-based slot, overflow folded into the last entry).
    /// Length is `hunter_lab`'s `N_EXIT_METRIC_SLOTS` (8) — a `Vec` (not a fixed
    /// array) purely because that's what binds to the Postgres `INTEGER[]`
    /// column; every row has the same length. Empty on rows written before this
    /// column existed. See the run/group response's `X-Exit-Metric-Legend`
    /// header for what each slot names.
    pub n_exit_metrics_by_slot: Vec<i32>,
    pub n_exit_open: i32,
}

/// Per-token outcome when a single combo is re-simulated on a group's corpus
/// slice. Returned by the `GET …/token-results` drill-in endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ComboTokenResult {
    pub mint_address: String,
    pub symbol: String,
    pub fired: bool,
    pub pnl_sol: f32,
    pub pnl_pct: f32,
    pub holding_secs: i64,
    /// Exit reason string: `"TakeProfit"`, `"StopLoss"`, `"TrailingStop"`,
    /// `"Stall"`, `"TimeStop"`, `"LiquidityExit"`,
    /// `"Dead"` (force-closed at the last meaningful trade — token died silent),
    /// `"Open"` (still open at end of history, token still alive), or `"NoEntry"`.
    /// A metric-condition exit is the spaced `metric op value` detail label
    /// (`"stall > 3"`), not the bare `"Metrics"` code name — see
    /// `hunter_engine::event::format_metric_exit_label`.
    pub exit: String,
    /// Which of the rule's own authored exit conditions `exit` fired on (0-based,
    /// see `n_exit_metrics_by_slot` on [`GroupedSweepResult`]); `None` for every
    /// non-metric exit (or a metric exit whose slot wasn't resolved).
    pub exit_metric_slot: Option<u8>,
    // --- Simulation fill details (populated by single-combo re-sim) ---
    /// RFC3339 block time of the simulated entry fill; `None` when not fired.
    pub entry_time: Option<String>,
    /// Simulated entry fill price in SOL/token; `None` when not fired.
    pub entry_price: Option<f64>,
    /// Real `tx_signature` of the entry fill, resolved from the `trades` table by
    /// (mint, `entry_slot`, buy) after the re-sim — the slim `CorpusTrade` the sweep
    /// walks carries no signature. `None` when not fired or unresolved.
    pub entry_tx: Option<String>,
    /// Slot of the entry fill trade — the join key for `entry_tx`. Kept on the
    /// result so the handler can resolve the signature; not surfaced in the UI.
    #[serde(skip_serializing)]
    pub entry_slot: Option<u64>,
    /// RFC3339 block time of the simulated exit fill; `None` when open or not fired.
    pub exit_time: Option<String>,
    /// Simulated exit fill price in SOL/token; `None` when open or not fired.
    pub exit_price: Option<f64>,
    /// Real `tx_signature` of the exit fill, resolved like `entry_tx` but by
    /// (mint, `exit_slot`, sell). `None` when open, not fired, or unresolved.
    pub exit_tx: Option<String>,
    /// Slot of the exit fill trade — the join key for `exit_tx`.
    #[serde(skip_serializing)]
    pub exit_slot: Option<u64>,
    // --- Token metadata (batch-joined from tokens + tokens_info after sim) ---
    /// Token creation time (RFC3339); row-owned (excluded from `token`).
    pub created_at: Option<String>,
    /// All-time-high price from `tokens_info`; row-owned (excluded from `token`).
    pub ath_price: Option<f64>,
    /// Full shared token enrichment (`creator_wallet`, `market_cap`, `trade_count`,
    /// `is_migrated`, `cu_price`, …) — the same SSOT the Matched / Positions /
    /// Simulated tables use, attached server-side after the re-sim. Default until the
    /// batch join runs.
    #[serde(flatten)]
    pub token: crate::storage::token_enrichment::TokenEnrichment,
}

/// A group plus its ranked combo rows, handed to the repo's `save_run` as the
/// write unit (the repo links them via a freshly-minted group id).
pub struct GroupedSweepGroupWrite {
    pub group_index: i32,
    pub group_key: Value,
    pub token_count: i32,
    pub fired_count: i64,
    pub best_combo_id: i32,
    pub best_score: Option<f64>,
    pub best_expectancy_sol: f64,
    pub best_params: Value,
    pub results: Vec<GroupedSweepResult>,
    /// Option C: mint addresses of every token that fell into this group. Stored
    /// in DB so `list_token_results` can load only these N tokens cold instead of
    /// re-loading the entire corpus.
    pub mints: Vec<String>,
}
