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
    pub exit_price: Option<f64>,
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
/// - `Current` — the rule's latest run only (the "Current run" section).
/// - `History` — every prior run (all runs except the latest; the "Old runs" section).
/// - `All`     — every run for the rule (real + paper); rows stamped with `run_seq`.
/// - `Run`     — one run selected by the `run_seq` query param.
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

#[derive(Deserialize)]
pub struct ScopeParam {
    pub scope: Option<PositionScope>,
    /// Required when `scope=run`.
    pub run_seq: Option<i64>,
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
    scope: Option<PositionScope>,
    run_seq: Option<i64>,
    body: TableRequest,
) -> HttpResponse {
    let (limit, offset) = body.pagination.bounds();
    let pq = PositionQuery::from(body);
    let repo = strategy_repo;

    // The rule's trade_mode drives run selection.
    let rule = match rule_repo.find(rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return json_positions_with_total(Vec::new(), 0),
        Err(e) => return list_error("load rule", e),
    };

    // Page the rows and count the (filtered) population for the pager. The scope
    // selects which run(s):
    //   `current` — the rule's latest run only (both modes);
    //   `history` — every prior run, each row stamped with its `run_seq`;
    //   `all`     — every run (incl. current), rows stamped with `run_seq`;
    //   `run`     — one run by `run_seq` query param;
    //   absent    — legacy: paper = latest run, real = full lifetime history.
    let (result, total, seq_map) = match scope {
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
            let runs = match repo.run_seqs_for_rule(rule_id, &rule.trade_mode).await {
                Ok(runs) => runs,
                Err(e) => return list_error("load runs", e),
            };
            // Need a current run to exclude AND at least one prior run for there to
            // be any history — otherwise the "old runs" section is empty.
            let Some(&(latest_run_id, _)) = runs.first() else {
                return json_positions_with_total(Vec::new(), 0);
            };
            if runs.len() <= 1 {
                return json_positions_with_total(Vec::new(), 0);
            }
            let seq_map: HashMap<Uuid, i64> = runs.into_iter().collect();
            (
                repo.find_positions_by_rule_excluding_run_paged(
                    rule_id,
                    latest_run_id,
                    limit,
                    offset,
                    &pq,
                )
                .await,
                repo.count_positions_by_rule_excluding_run(rule_id, latest_run_id, &pq).await,
                Some(seq_map),
            )
        }
        Some(PositionScope::All) => {
            let runs = match repo.run_seqs_for_rule(rule_id, &rule.trade_mode).await {
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
            let Some(seq) = run_seq else {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": "scope=run requires run_seq"}));
            };
            match repo.find_run_by_seq(rule_id, &rule.trade_mode, seq).await {
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
    scope: Option<PositionScope>,
    run_seq: Option<i64>,
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
    let result = match scope {
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
            Ok(None) => Ok(PositionsSummary::default()),
            Err(e) => return list_error("load current run", e),
        },
        Some(PositionScope::All) => repo.positions_summary_by_rule(rule_id, &pq, price_of).await,
        Some(PositionScope::Run) => {
            let Some(seq) = run_seq else {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": "scope=run requires run_seq"}));
            };
            match repo.find_run_by_seq(rule_id, &rule.trade_mode, seq).await {
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

#[derive(Deserialize)]
pub struct ScoreScopeParam {
    pub score_scope: Option<ScoreScope>,
}

/// `GET /strategy-rules?score_scope=current|all` — every rule with its position
/// counters folded in (the columns the Rules list scores on: `total_pnl_sol`,
/// `avg_pnl_pct`, `win_rate`, `win_count`/`loss_count`, `total_positions`, and the
/// open/pending split).
///
/// Both bins serve this. On `live` the counters roll up the positions its engine is
/// writing; on `lab` they roll up the same table as synced from EC2 — so a rule
/// scores the same on both boards, and the analysis app can rank/compare real
/// performance without a second scoring implementation to keep in step.
///
/// A counters lookup failure degrades to zeros for that mode rather than failing the
/// list — a scoreboard column going blank beats the Rules page not rendering.
pub async fn rules_with_counters(
    strategy_repo: &StrategyRepo,
    rule_repo: &RuleRepo,
    score_scope: ScoreScope,
) -> HttpResponse {
    let rules = match rule_repo.list().await {
        Ok(v) => v,
        Err(e) => return list_error("list rules", e),
    };
    let (paper, real) = match score_scope {
        ScoreScope::Current => (
            strategy_repo
                .rule_counters_for_latest_runs("generic", "paper")
                .await
                .unwrap_or_default(),
            strategy_repo
                .rule_counters_for_latest_runs("generic", "real")
                .await
                .unwrap_or_default(),
        ),
        ScoreScope::All => (
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
                map.insert("avg_pnl_pct".into(), serde_json::json!(c.avg_pnl_pct));
                map.insert("total_pnl_sol".into(), serde_json::json!(c.total_pnl_sol));
                map.insert(
                    "score_scope".into(),
                    serde_json::json!(match score_scope {
                        ScoreScope::Current => "current",
                        ScoreScope::All => "all",
                    }),
                );
            }
            v
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// `GET /strategy-rules/{id}/runs` — the rule's run history with finalized metrics,
/// newest first. Drives the Evidence run navigator (chips + cross-run PnL trend).
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
    match strategy_repo.list_runs_with_metrics(rule_id, &rule.trade_mode).await {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => list_error("list rule runs", e),
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
