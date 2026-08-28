//! Per-rule position **reads** — the one implementation shared by both bins.
//!
//! `live` serves these off its own `strategy_positions` (the rows its engine is
//! writing right now); `lab` serves the same shape off the synced local mirror
//! (`scripts/db-incremental-sync.ps1` copies `strategy_rules` / `strategy_runs` /
//! `strategy_run_metrics` / `strategy_positions`), so the analysis app can inspect
//! real/paper positions with the lab-only metric panes.
//!
//! SSOT: the run-scope semantics (`current` / `history` / `all` / `run` × the
//! paper-vs-real legacy default) and the wire shape live **here**, not in either
//! bin. Two copies of this match would drift silently — a lab table paging one
//! population while the live table pages another is exactly the bug that would
//! never surface until the numbers disagreed.
//!
//! Both bins own only the state plumbing: pull `StrategyRepo` + `RuleRepo` +
//! a price lookup out of their own state struct and call [`rule_positions_page`] /
//! [`rule_positions_summary`].
//!
//! [`rules_with_counters`] belongs here for the same reason: the rule-list scoreboard
//! (PnL / Avg% / Win% / W-L / N) is a **position** rollup, not a live-engine fact, so
//! both apps score a rule identically off whichever `strategy_positions` they can see.

use actix_web::HttpResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::api::table_query::TableRequest;
use crate::models::{PositionsSummary, StrategyPosition};
use crate::storage::repositories::{
    rule_repo::RuleRepo,
    strategy_repo::{PositionQuery, StrategyRepo},
};

// ---------------------------------------------------------------------------
// Wire type
// ---------------------------------------------------------------------------

/// Wire shape for a position read. Field set is kept stable for the frontend; the
/// JSONB signature arrays are decoded to `Vec<String>` and the single-address
/// display columns are the first entry leg / last exit leg.
///
/// SSOT NOTE: this is the ONE position wire struct — the frontend's
/// `RulePositionRecord` mirrors it field-for-field, and both bins serialize
/// through it. A new field lands here once.
#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub run_id: Uuid,
    pub mint_address: String,
    pub wallet: String,
    /// Target (trigger-trade) snapshot — the scalp-entry signal trade that armed
    /// this position, distinct from the actual entry fill. `None` for strategies
    /// that never arm.
    pub target_price: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    pub entry_price: Option<f64>,
    /// SOL-weighted **average** across every sell leg on a scale-out — not a price
    /// anything filled at. Charts draw `exit_legs` when it is populated and fall
    /// back to this only for a single-leg close, where the two are the same number.
    pub exit_price: Option<f64>,
    /// Per-leg sell fills, in fire order, wherever `exit_price` alone cannot describe
    /// them: a ladder (where it is an average) or a still-open position that has banked
    /// a leg (where there is no stamp yet). See `StrategyRepo::sell_legs_for_positions`
    /// for the exact rule. Attached per page by
    /// [`attach_exit_legs`], so — like enrichment — the SSE-delta / single-position
    /// paths leave it empty and the client keeps the legs it already holds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exit_legs: Vec<crate::models::ExitFillLeg>,
    /// First entry leg's signature (display/back-compat); empty until the fill is
    /// adopted. The full per-leg list is `entry_tx_signatures`.
    pub entry_tx: String,
    /// Last exit leg's signature (display/back-compat); `None` until a sell lands.
    /// The full per-leg list is `exit_tx_signatures`.
    pub exit_tx: Option<String>,
    pub entry_tx_signatures: Vec<String>,
    pub exit_tx_signatures: Vec<String>,
    pub status: String,
    pub strategy: String,
    /// Execution mode (`real` | `paper`) — the cross-rule History view mixes both,
    /// so rows carry it (per-rule views infer it from the rule).
    pub mode: String,
    /// Owning rule (`None` if the rule was deleted — `ON DELETE SET NULL`).
    pub rule_id: Option<Uuid>,
    /// Entry cost in human SOL (from `entry_lamports`) — the History table's
    /// Entry ◎ column and the `pnlPctFromSol` denominator.
    pub entry_sol: Option<f64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub entry_token_amount: Option<u64>,
    /// Raw token units (exact integer; the frontend scales for display).
    pub exit_token_amount: Option<u64>,
    /// Running sum of confirmed sell-leg raw token units (scale-out; mig 0018).
    pub sold_token_amount: u64,
    /// Running sum of confirmed sell-leg SOL (human); from `exit_sol_lamports_total`.
    pub exit_sol_total: f64,
    /// Next scale-out stage index (`0` = pre-first / legacy).
    pub scale_stage: u8,
    /// Sold fraction of the initial bag in bps (`sold * 10_000 / entry`).
    pub sold_bps: u16,
    pub pnl_percent: Option<f64>,
    /// Realized SOL PnL (`StrategyPosition::realized_pnl_sol`) — the canonical
    /// win/loss basis mirroring `positions_summary`/`is_win`.
    pub pnl_sol: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_reason: Option<String>,
    /// Owning run's monotonic sequence (`strategy_runs.run_seq`). Populated only by
    /// the multi-run views — where it drives the run column + banding; `None` on the
    /// current-run/live paths (single run) and SSE deltas.
    pub run_seq: Option<i64>,
    /// `bot` | `manual` — who opened the position (Console origin dot / filter).
    pub origin: String,
    /// Manual-position TP/SL config (`{"tp_pct", "sl_pct"}`); `None` on bot rows
    /// and tracked-only manual rows.
    pub manual_exit: Option<serde_json::Value>,
    /// True on a stale, unresolved `BuySubmitted` (no fill adopted, not proven
    /// reverted, older than the review window) — the row needs a manual Verify
    /// (B3). Derived server-side so the UI never infers it from timestamps.
    pub needs_review: bool,
    /// Reaper redrive state on an `ExitStuck` row: parked ⇒ auto-retry stopped
    /// (cap hit), waiting on a manual Retry / Dump / Write-off.
    pub exit_parked: bool,
    pub exit_redrive_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Token symbol (row-owned identity; excluded from the shared `token` flatten).
    /// Empty until enriched.
    pub symbol: String,
    /// Token all-time-high price (`tokens_info`; row-owned, excluded from `token`).
    pub ath_price: Option<f64>,
    /// Full shared token enrichment (`name`, `market_cap`, `cu_price`, `trade_count`,
    /// `is_migrated`, …) — the same SSOT the Matched / Simulated / Sweep tables use,
    /// attached server-side from `strategy_positions LEFT JOIN tokens`'s mints so the
    /// positions table sorts/filters/searches on token columns with no client merge.
    /// Default (empty) on the SSE-delta / single-position paths (the client already
    /// holds the token there).
    #[serde(flatten)]
    pub token: crate::storage::token_enrichment::TokenEnrichment,
}

impl From<StrategyPosition> for PositionResponse {
    fn from(p: StrategyPosition) -> Self {
        let pnl_percent = p.pnl_pct();
        let pnl_sol = p.realized_pnl_sol();
        let sold_bps = p.sold_bps();
        let entry_sigs = p.entry_tx_sigs();
        let exit_sigs = p.exit_tx_sigs();
        Self {
            id: p.id,
            run_id: p.run_id,
            mint_address: p.mint_address,
            wallet: p.wallet,
            target_price: p.target_price,
            target_token_amount: p.target_token_amount,
            target_time: p.target_time,
            target_tx: p.target_tx,
            entry_price: p.entry_price,
            exit_price: p.exit_price,
            // Legs are attached by the paged handler (`attach_exit_legs`); empty here
            // so the SSE-delta / single-position paths stay one row, one query.
            exit_legs: Vec::new(),
            entry_tx: entry_sigs.first().cloned().unwrap_or_default(),
            exit_tx: exit_sigs.last().cloned(),
            entry_tx_signatures: entry_sigs,
            exit_tx_signatures: exit_sigs,
            status: p.status,
            strategy: p.strategy_id,
            mode: p.mode,
            rule_id: p.rule_id,
            entry_sol: p.entry_sol,
            entry_token_amount: p.entry_token_amount,
            exit_token_amount: p.exit_token_amount,
            sold_token_amount: p.sold_token_amount,
            exit_sol_total: p.exit_sol_total,
            scale_stage: p.scale_stage,
            sold_bps,
            pnl_percent,
            pnl_sol,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            exit_reason: p.exit_reason,
            // Stamped by the paged handler from the run map; single-run views leave
            // it None.
            run_seq: None,
            origin: p.origin,
            manual_exit: p.manual_exit,
            needs_review: p.needs_review,
            exit_parked: p.exit_parked,
            exit_redrive_count: p.exit_redrive_count,
            created_at: p.created_at,
            updated_at: p.updated_at,
            // Enrichment is attached by the paged handler (`enrich_position_responses`);
            // default here so the SSE-delta / single-position paths stay unchanged.
            symbol: String::new(),
            ath_price: None,
            token: Default::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Scope selectors
// ---------------------------------------------------------------------------

/// Run-split selector for the by-rule positions + summary views.
/// - `Current` — the latest run **in the rule's own mode** (the "Current run" section).
/// - `History` — every run except that one, across both modes ("Old runs"). With no
///   run in the rule's own mode there is no current run to exclude, so this is every
///   run the rule has.
/// - `All`     — every run for the rule (real + paper); rows stamped with `run_seq`.
/// - `Run`     — one run selected by `run_seq` + the `mode` it belongs to.
///
/// Only `Current` is mode-scoped: a rule owns its whole position history regardless
/// of which mode recorded it, and `trade_mode` is a live switch — scoping the history
/// to it would hide every position the rule took in the mode it is no longer in.
///
/// Absent (`None`) preserves the legacy behavior (paper = latest run, real = all runs)
/// for any caller that doesn't opt into the split.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PositionScope {
    Current,
    History,
    All,
    Run,
}

/// `?mode=real|paper` for the mint-episode overlay. Absent ⇒ `real`; real and paper
/// never share a chart (see [`mint_episodes`]).
#[derive(Deserialize)]
pub struct EpisodeModeParam {
    pub mode: Option<String>,
}

#[derive(Deserialize)]
pub struct ScopeParam {
    pub scope: Option<PositionScope>,
    /// Required when `scope=run`.
    pub run_seq: Option<i64>,
    /// Which mode's `run_seq` — `run_seq` is monotonic per `(rule_id, mode)`, so
    /// `#3` names two different runs once a rule has traded in both. Absent ⇒ the
    /// rule's own `trade_mode`. Only `scope=run` reads it.
    pub mode: Option<String>,
}

impl ScopeParam {
    /// The mode `scope=run` resolves `run_seq` in: the requested one when it names a
    /// real mode, else the rule's own. Unknown text falls back rather than 400s — a
    /// stale link should degrade to the rule's mode, not break the panel.
    fn run_mode<'a>(&'a self, rule_mode: &'a str) -> &'a str {
        match self.mode.as_deref() {
            Some("real") => "real",
            Some("paper") => "paper",
            _ => rule_mode,
        }
    }
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Attach shared token enrichment to a page of position responses via one bounded
/// batch fetch (`token_enrichment::fetch_by_mints` over the page's mints) — the same
/// SSOT the Matched / Simulated / Sweep tables use. Sets the row-owned `symbol` /
/// `ath_price` off the row too. A fetch error is logged and leaves rows un-enriched
/// (the table still renders; enrichment columns are just blank) rather than failing
/// the whole list.
pub async fn enrich_position_responses(repo: &StrategyRepo, responses: &mut [PositionResponse]) {
    if responses.is_empty() {
        return;
    }
    let mints: Vec<String> = responses.iter().map(|r| r.mint_address.clone()).collect();
    match crate::storage::token_enrichment::fetch_by_mints(repo.pool(), &mints).await {
        Ok(rows) => {
            let by_mint: HashMap<String, _> =
                rows.into_iter().map(|r| (r.mint_address.clone(), r)).collect();
            for r in responses.iter_mut() {
                if let Some(row) = by_mint.get(&r.mint_address) {
                    r.symbol = row.symbol.clone();
                    r.ath_price = row.ath_price;
                    r.token = row.into();
                }
            }
        }
        Err(e) => tracing::warn!("positions enrichment fetch failed: {e}"),
    }
}

/// Attach per-leg exit fills to a page of position responses via one bounded batch
/// fetch — the scale-out twin of [`enrich_position_responses`], and for the same
/// reason: every row on the page can be drawn as a chart card, so a per-row query
/// would be an N+1 on the History deck.
///
/// A fetch error is logged and leaves the arrays empty: the charts then fall back to
/// the position-level `exit_*` single arrow, which is exactly the pre-legs behavior.
pub async fn attach_exit_legs(repo: &StrategyRepo, responses: &mut [PositionResponse]) {
    if responses.is_empty() {
        return;
    }
    let ids: Vec<Uuid> = responses.iter().map(|r| r.id).collect();
    match repo.sell_legs_for_positions(&ids).await {
        Ok(mut by_position) => {
            for r in responses.iter_mut() {
                if let Some(legs) = by_position.remove(&r.id) {
                    r.exit_legs = legs;
                }
            }
        }
        Err(e) => tracing::warn!("positions exit-leg fetch failed: {e}"),
    }
}

/// Stamp the pager total on `X-Total-Count` (and expose it to the browser fetch,
/// needed when the SPA is served through the dev proxy / a different origin) —
/// the JSON body stays the plain array-of-positions contract.
fn json_positions_with_total(positions: Vec<StrategyPosition>, total: i64) -> HttpResponse {
    let responses: Vec<PositionResponse> =
        positions.into_iter().map(PositionResponse::from).collect();
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}

/// Build + enrich + serialize a page of positions with the pager total, stamping
/// each row's `run_seq` from a `run_id → run_seq` map (a `None` map leaves it unset).
async fn json_positions_enriched(
    repo: &StrategyRepo,
    positions: Vec<StrategyPosition>,
    total: i64,
    seq_map: Option<&HashMap<Uuid, i64>>,
) -> HttpResponse {
    let mut responses: Vec<PositionResponse> = positions
        .into_iter()
        .map(|p| {
            let mut r = PositionResponse::from(p);
            if let Some(map) = seq_map {
                r.run_seq = map.get(&r.run_id).copied();
            }
            r
        })
        .collect();
    enrich_position_responses(repo, &mut responses).await;
    attach_exit_legs(repo, &mut responses).await;
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}

pub(crate) fn list_error(what: &str, e: anyhow::Error) -> HttpResponse {
    tracing::error!("Failed to {what}: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({"error": "Failed to load positions"}))
}

// ---------------------------------------------------------------------------
// The two shared reads
// ---------------------------------------------------------------------------

/// `POST /rules/{rule_id}/positions` — one page of a rule's positions.
///
/// Paper rules retain only the current run's bag, so the scope-less default serves
/// them from the rule's latest paper run; real rules carry their full lifetime
/// history. `body` is the unified [`TableRequest`] (paging + server-side
/// sort/search/filter), applied to both the page and the count so the pager sizes
/// against the same filtered population.
pub async fn rule_positions_page(
    strategy_repo: &StrategyRepo,
    rule_repo: &RuleRepo,
    rule_id: Uuid,
    params: ScopeParam,
    body: TableRequest,
) -> HttpResponse {
    let (limit, offset) = body.pagination.bounds();
    let pq = PositionQuery::from(body);
    let repo = strategy_repo;

    // The rule's trade_mode picks the *current* run; every history scope spans modes.
    let rule = match rule_repo.find(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return json_positions_with_total(Vec::new(), 0),
        Err(e) => return list_error("load rule", e),
    };

    // Page the rows and count the (filtered) population for the pager. The scope
    // selects which run(s):
    //   `current` — the latest run in the rule's own mode;
    //   `history` — every other run, each row stamped with its `run_seq`;
    //   `all`     — every run (incl. current), rows stamped with `run_seq`;
    //   `run`     — one run by `run_seq` + `mode`;
    //   absent    — legacy: paper = latest run, real = full lifetime history.
    let (result, total, seq_map) = match params.scope {
        Some(PositionScope::Current) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            Ok(Some(run)) => (
                repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                repo.count_positions_by_run(run.id, &pq).await,
                None,
            ),
            Ok(None) => return json_positions_with_total(Vec::new(), 0),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::History) => {
            // Runs across BOTH modes: the page below is scoped by `rule_id`, so a
            // mode-filtered map would blank the run stamp on every row the rule
            // recorded in its other mode.
            let runs = match repo.run_seqs_for_rule(rule_id, None).await {
                Ok(runs) => runs,
                Err(e) => return list_error("load runs", e),
            };
            if runs.is_empty() {
                return json_positions_with_total(Vec::new(), 0);
            }
            // The run to exclude. A rule whose own `trade_mode` has no run at all
            // has no *current* run, so there is nothing to exclude and every run it
            // recorded in the other mode is history — excluding-nothing is exactly
            // the `All` population. Returning empty here instead blanked the section
            // for any rule flipped into a mode it has never traded in.
            let current_run_id = match repo.latest_run(rule_id, &rule.trade_mode).await {
                Ok(run) => run.map(|r| r.id),
                Err(e) => return list_error("load current run", e),
            };
            // With a current run, a lone run means that run IS the whole history.
            if current_run_id.is_some() && runs.len() <= 1 {
                return json_positions_with_total(Vec::new(), 0);
            }
            let seq_map: HashMap<Uuid, i64> = runs.into_iter().collect();
            let (rows, total) = match current_run_id {
                Some(exclude) => (
                    repo.find_positions_by_rule_excluding_run_paged(
                        rule_id, exclude, limit, offset, &pq,
                    )
                    .await,
                    repo.count_positions_by_rule_excluding_run(rule_id, exclude, &pq).await,
                ),
                None => (
                    repo.find_positions_by_rule_paged(rule_id, limit, offset, &pq).await,
                    repo.count_positions_by_rule(rule_id, &pq).await,
                ),
            };
            (rows, total, Some(seq_map))
        }
        Some(PositionScope::All) => {
            // Cross-mode for the same reason as `History` — all-time means all-time.
            let runs = match repo.run_seqs_for_rule(rule_id, None).await {
                Ok(runs) => runs,
                Err(e) => return list_error("load runs", e),
            };
            let seq_map: HashMap<Uuid, i64> = runs.into_iter().collect();
            (
                repo.find_positions_by_rule_paged(rule_id, limit, offset, &pq).await,
                repo.count_positions_by_rule(rule_id, &pq).await,
                Some(seq_map),
            )
        }
        Some(PositionScope::Run) => {
            let Some(seq) = params.run_seq else {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": "scope=run requires run_seq"}));
            };
            match repo.find_run_by_seq(rule_id, params.run_mode(&rule.trade_mode), seq).await {
                Ok(Some(run)) => {
                    let mut seq_map = HashMap::new();
                    seq_map.insert(run.id, run.run_seq);
                    (
                        repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                        repo.count_positions_by_run(run.id, &pq).await,
                        Some(seq_map),
                    )
                }
                Ok(None) => return json_positions_with_total(Vec::new(), 0),
                Err(e) => return list_error("load run by seq", e),
            }
        }
        None if rule.trade_mode == "paper" => match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => (
                repo.find_positions_by_run_paged(run.id, limit, offset, &pq).await,
                repo.count_positions_by_run(run.id, &pq).await,
                None,
            ),
            Ok(None) => (Ok(Vec::new()), Ok(0), None),
            Err(e) => return list_error("load paper run", e),
        },
        None => (
            repo.find_positions_by_rule_paged(rule_id, limit, offset, &pq).await,
            repo.count_positions_by_rule(rule_id, &pq).await,
            None,
        ),
    };

    match (result, total) {
        (Ok(positions), Ok(total)) => {
            json_positions_enriched(repo, positions, total, seq_map.as_ref()).await
        }
        (Err(e), _) | (_, Err(e)) => list_error("load positions for rule", e),
    }
}

/// `POST /portfolio/positions/query` — one page of positions across **all rules
/// and runs** (the Console History table). Same [`TableRequest`] wire contract and
/// SQL machinery as the per-rule read; the cohort narrows only through the body's
/// filters (`mode` / `rule_id` / `status` / `exit_reason`, `In`-capable) and its
/// `range` (the close-or-entry time window). No run-scope semantics here — History
/// spans runs by design.
pub async fn portfolio_positions_page(
    strategy_repo: &StrategyRepo,
    body: TableRequest,
) -> HttpResponse {
    let (limit, offset) = body.pagination.bounds();
    let pq = PositionQuery::from(body);
    match (
        strategy_repo.find_positions_all_paged(limit, offset, &pq).await,
        strategy_repo.count_positions_all(&pq).await,
    ) {
        (Ok(positions), Ok(total)) => {
            json_positions_enriched(strategy_repo, positions, total, None).await
        }
        (Err(e), _) | (_, Err(e)) => list_error("load portfolio positions", e),
    }
}

/// `POST /rules/{rule_id}/positions/summary` — position aggregates over the same
/// filtered population [`rule_positions_page`] pages (pagination/sort ignored), with
/// the same win/closed/open semantics as the per-rule runtime counters.
///
/// `price_of` marks the still-open positions to market for `open_pnl_sol`. The live
/// bin passes its in-memory token cache (no DB or RPC round-trip); the lab bin passes
/// its own seeded cache. `None` for a token with no price leaves that position out of
/// the mark rather than inventing one.
pub async fn rule_positions_summary<F>(
    strategy_repo: &StrategyRepo,
    rule_repo: &RuleRepo,
    rule_id: Uuid,
    params: ScopeParam,
    body: TableRequest,
    price_of: F,
) -> HttpResponse
where
    F: Fn(&str) -> Option<f64> + Copy,
{
    let repo = strategy_repo;
    let pq = PositionQuery::from(body);

    let rule = match rule_repo.find(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return HttpResponse::Ok().json(PositionsSummary::default()),
        Err(e) => return list_error("load rule", e),
    };

    // Mirror the scope semantics of `rule_positions_page` so the summary card
    // aggregates exactly the population its table pages.
    let result = match params.scope {
        Some(PositionScope::Current) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq, price_of).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::History) => match repo.latest_run(rule_id, &rule.trade_mode).await {
            // Exclude the current run; a lone run yields an empty (tokens=0) summary.
            Ok(Some(run)) => {
                repo.positions_summary_by_rule_excluding_run(rule_id, run.id, &pq, price_of).await
            }
            // No run in the rule's own mode ⇒ no current run to exclude ⇒ every run
            // it recorded is history. Mirrors the page, which is the whole point of
            // this match existing.
            Ok(None) => repo.positions_summary_by_rule(rule_id, &pq, price_of).await,
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::All) => repo.positions_summary_by_rule(rule_id, &pq, price_of).await,
        Some(PositionScope::Run) => {
            let Some(seq) = params.run_seq else {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": "scope=run requires run_seq"}));
            };
            match repo.find_run_by_seq(rule_id, params.run_mode(&rule.trade_mode), seq).await {
                Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq, price_of).await,
                Ok(None) => Ok(PositionsSummary::default()),
                Err(e) => return list_error("load run by seq", e),
            }
        }
        None if rule.trade_mode == "paper" => match repo.latest_run(rule_id, "paper").await {
            Ok(Some(run)) => repo.positions_summary_by_run(run.id, &pq, price_of).await,
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load paper run", e),
        },
        None => repo.positions_summary_by_rule(rule_id, &pq, price_of).await,
    };

    match result {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => list_error("load positions summary", e),
    }
}

/// `POST /portfolio/positions/summary` — the aggregate twin of
/// [`portfolio_positions_page`]: same [`TableRequest`] body, same
/// [`PositionQuery`], no run-scope semantics, so the Console History summary
/// strip totals exactly the population its table pages (pagination/sort ignored).
///
/// Aggregating server-side rather than folding the fetched rows is what lets the
/// strip stay exact past the page size — the table shows 25 of n, and a summary
/// computed from those 25 would silently re-state itself on every page turn.
///
/// `price_of` marks the still-open positions for `open_pnl_sol` — see
/// [`rule_positions_summary`].
pub async fn portfolio_positions_summary<F>(
    strategy_repo: &StrategyRepo,
    body: TableRequest,
    price_of: F,
) -> HttpResponse
where
    F: Fn(&str) -> Option<f64> + Copy,
{
    let pq = PositionQuery::from(body);
    match strategy_repo.positions_summary_all(&pq, price_of).await {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => list_error("load portfolio positions summary", e),
    }
}

// ---------------------------------------------------------------------------
// Rule-list scoreboard
// ---------------------------------------------------------------------------

/// Which population the rule-list scoreboard scores over.
/// - `All`     — real = all-time positions; paper = latest run (legacy scoreboard)
/// - `Current` — latest run for **both** modes (the keep/kill board)
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScoreScope {
    Current,
    All,
}

/// Which trade mode the rule-list scoreboard scores every rule in.
///
/// Absent is NOT the same as either value: it scores each rule in its **own**
/// `trade_mode`, which is the board you keep/kill on. An explicit mode scores every
/// rule in that one mode instead — the only way to read a rule's paper record while
/// it trades real, or to rank the whole list on one comparable basis.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScoreMode {
    Paper,
    Real,
}

impl ScoreMode {
    fn as_str(self) -> &'static str {
        match self {
            ScoreMode::Paper => "paper",
            ScoreMode::Real => "real",
        }
    }
}

#[derive(Deserialize)]
pub struct ScoreScopeParam {
    pub score_scope: Option<ScoreScope>,
    /// Absent ⇒ score each rule in its own `trade_mode` (the default board).
    pub score_mode: Option<ScoreMode>,
}

/// `GET /strategy-rules?score_scope=current|all` — every rule with its position
/// counters folded in (the columns the Rules list scores on: `total_pnl_sol`,
/// `return_pct` + its `closed_entry_sol` denominator, `win_rate`,
/// `win_count`/`loss_count`, `total_positions`, and the open/pending split).
///
/// Both bins serve this. On `live` the counters roll up the positions its engine is
/// writing; on `lab` they roll up the same table as synced from EC2 — so a rule
/// scores the same on both boards, and the analysis app can rank/compare real
/// performance without a second scoring implementation to keep in step.
///
/// `score_mode` pins every rule to one mode's counters instead of its own — paper
/// and real are separate ledgers (paper pays no slippage a real fill pays), so a
/// board mixing them ranks on a number that means two different things.
///
/// A counters lookup failure degrades to zeros for that mode rather than failing the
/// list — a scoreboard column going blank beats the Rules page not rendering.
pub async fn rules_with_counters(
    strategy_repo: &StrategyRepo,
    rule_repo: &RuleRepo,
    score_scope: ScoreScope,
    score_mode: Option<ScoreMode>,
) -> HttpResponse {
    let rules = match rule_repo.list().await {
        Ok(v) => v,
        Err(e) => return list_error("list rules", e),
    };
    // Config changes that landed while a rule's current run was scoring (mig 0012).
    // A rule edited while active keeps its run, so without this the board shows one
    // set of numbers for two configs and says nothing about it. Degrades to "no
    // marker" like the counters do — a missing badge beats a blank Rules page.
    let config_edits = strategy_repo
        .running_run_config_edits("generic")
        .await
        .unwrap_or_default();
    // One map when a mode is pinned (every rule reads from it), two when each rule
    // is scored in its own mode. `All` + a pinned mode is all-time on BOTH sides —
    // the paper/latest-run asymmetry below is the legacy default board's, and
    // carrying it into an explicit comparison would silently compare unlike spans.
    let (paper, real) = match (score_mode, score_scope) {
        (Some(m), ScoreScope::Current) => {
            let c = strategy_repo
                .rule_counters_for_latest_runs("generic", m.as_str())
                .await
                .unwrap_or_default();
            (c.clone(), c)
        }
        (Some(m), ScoreScope::All) => {
            let c = strategy_repo
                .rule_counters_for_all_in_mode(m.as_str())
                .await
                .unwrap_or_default();
            (c.clone(), c)
        }
        (None, ScoreScope::Current) => (
            strategy_repo
                .rule_counters_for_latest_runs("generic", "paper")
                .await
                .unwrap_or_default(),
            strategy_repo
                .rule_counters_for_latest_runs("generic", "real")
                .await
                .unwrap_or_default(),
        ),
        (None, ScoreScope::All) => (
            strategy_repo
                .rule_counters_for_latest_paper_runs("generic")
                .await
                .unwrap_or_default(),
            strategy_repo.rule_counters_for_all_real().await.unwrap_or_default(),
        ),
    };
    let out: Vec<serde_json::Value> = rules
        .into_iter()
        .map(|r| {
            // With a mode pinned both maps are the same one, so the rule's own
            // `trade_mode` stops steering which ledger it is scored on.
            let counters = if r.trade_mode == "paper" {
                paper.get(&r.id)
            } else {
                real.get(&r.id)
            };
            let mut v = serde_json::to_value(&r).unwrap_or_else(|_| serde_json::json!({}));
            if let serde_json::Value::Object(map) = &mut v {
                let c = counters.cloned().unwrap_or_default();
                map.insert("total_positions".into(), serde_json::json!(c.total_positions));
                map.insert("open_positions".into(), serde_json::json!(c.open_positions));
                map.insert(
                    "pending_positions".into(),
                    serde_json::json!(c.pending_positions),
                );
                map.insert("win_count".into(), serde_json::json!(c.win_count));
                map.insert("loss_count".into(), serde_json::json!(c.loss_count));
                map.insert("win_rate".into(), serde_json::json!(c.win_rate));
                map.insert("return_pct".into(), serde_json::json!(c.return_pct));
                map.insert("total_pnl_sol".into(), serde_json::json!(c.total_pnl_sol));
                // `return_pct`'s own denominator — the Rules TOTAL tile re-weights
                // by capital across rules, which a per-rule percent cannot do.
                map.insert("closed_entry_sol".into(), serde_json::json!(c.closed_entry_sol));
                map.insert(
                    "score_scope".into(),
                    serde_json::json!(match score_scope {
                        ScoreScope::Current => "current",
                        ScoreScope::All => "all",
                    }),
                );
                // Which ledger the numbers above came from — `null` = the rule's own
                // `trade_mode`. Stamped on the row (like `score_scope`) so a board
                // is self-describing: the numbers say which basis produced them
                // rather than relying on what the caller believes it asked for.
                map.insert(
                    "score_mode".into(),
                    serde_json::json!(score_mode.map(ScoreMode::as_str)),
                );
                // Keyed by the rule's OWN mode, never the scored one: the marker is
                // about the run that is live right now, which exists in one mode.
                if let Some(edits) = config_edits.get(&(r.id, r.trade_mode.clone())) {
                    map.insert("config_edits".into(), edits.clone());
                }
            }
            v
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// `GET /strategy-rules/{id}/runs` — the rule's run history with finalized metrics.
/// Drives the Evidence run navigator (chips + cross-run PnL trend).
///
/// Both modes, ordered **rule's own mode first**, newest-first within each — so the
/// head of the list is the current run whatever the rule was flipped to, and the
/// runs from the other mode stay reachable instead of disappearing with the toggle.
/// Every row carries its `mode`, which is what disambiguates the per-mode `run_seq`
/// the client sends back as `scope=run`.
pub async fn rule_runs(
    strategy_repo: &StrategyRepo,
    rule_repo: &RuleRepo,
    rule_id: Uuid,
) -> HttpResponse {
    let rule = match rule_repo.find(rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "rule not found"}))
        }
        Err(e) => return list_error("load rule", e),
    };
    match strategy_repo.list_runs_with_metrics(rule_id, None).await {
        Ok(mut rows) => {
            // Stable, so the repo's `(mode, run_seq DESC)` order survives inside
            // each block.
            rows.sort_by_key(|r| r.mode != rule.trade_mode);
            HttpResponse::Ok().json(rows)
        }
        Err(e) => list_error("list rule runs", e),
    }
}

/// Cap on episodes overlaid on one token chart. Re-entry is capped per run, so a
/// mint reaching this has a pathological rule, not a legible chart.
const MAX_MINT_EPISODES: i64 = 200;

/// `GET /strategies/{strategy}/positions/mint/{mint}/episodes?mode=real` — every
/// entered episode on one mint, oldest first, legs attached.
///
/// The chart's whole trade history for a token: N entries and N exit ladders, not
/// just the one row that was clicked. Both bins serve it so Console/Evidence (live)
/// and Evidence (lab mirror) overlay the same set; `mode` defaults to `real`.
pub async fn mint_episodes(
    strategy_repo: &StrategyRepo,
    mint: &str,
    mode: Option<&str>,
) -> HttpResponse {
    let mode = match mode {
        Some("paper") => "paper",
        _ => "real",
    };
    match strategy_repo.find_entered_episodes_by_mint(mint, mode, MAX_MINT_EPISODES).await {
        Ok(positions) => {
            let mut responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            // Legs, but no token enrichment: these rows draw markers, they never
            // populate a table, so the enrichment batch would be a wasted round trip.
            attach_exit_legs(strategy_repo, &mut responses).await;
            HttpResponse::Ok().json(responses)
        }
        Err(e) => list_error("load mint episodes", e),
    }
}

/// `GET /strategies/{strategy}/positions/{position_id}/fills` — append-only
/// fill ledger for one episode (entry + every sell leg). Both bins serve this
/// so Console (live) and Evidence (lab mirror) share one wire shape.
pub async fn position_fills(strategy_repo: &StrategyRepo, position_id: Uuid) -> HttpResponse {
    match strategy_repo.find_position(position_id).await {
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "Position not found"}))
        }
        Err(e) => return list_error("load position", e),
        Ok(Some(_)) => {}
    }
    match strategy_repo.list_position_fills(position_id).await {
        Ok(fills) => HttpResponse::Ok().json(fills),
        Err(e) => list_error("list position fills", e),
    }
}

#[cfg(test)]
mod run_scope_tests {
    //! No-DB guard on the `scope=run` mode resolution. `run_seq` is monotonic per
    //! `(rule_id, mode)`, so the mode is half the key: resolving `#1` in the wrong
    //! mode serves a different run's positions under the run the user clicked.
    use super::*;

    fn param(mode: Option<&str>) -> ScopeParam {
        ScopeParam {
            scope: Some(PositionScope::Run),
            run_seq: Some(1),
            mode: mode.map(str::to_string),
        }
    }

    #[test]
    fn requested_mode_wins_over_the_rule_s_own() {
        // The whole point: a rule flipped to paper can still open its real runs.
        assert_eq!(param(Some("real")).run_mode("paper"), "real");
        assert_eq!(param(Some("paper")).run_mode("real"), "paper");
    }

    #[test]
    fn absent_or_unknown_mode_falls_back_to_the_rule() {
        assert_eq!(param(None).run_mode("paper"), "paper");
        // A stale link degrades to the rule's mode instead of 400-ing the panel.
        assert_eq!(param(Some("REAL")).run_mode("paper"), "paper");
        assert_eq!(param(Some("")).run_mode("real"), "real");
    }
}
