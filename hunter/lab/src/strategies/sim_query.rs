//! In-memory server-side query over a finished backtest's per-token results.
//!
//! The Simulated token table pages/sorts/filters/searches over the **unified**
//! `TableRequest` contract — same shape as Positions/Matched — but its data source
//! is the already-resident `Vec<Value>` in [`SimResults`](crate::state::sim_results)
//! (lab is single-user, workstation RAM), so there's no DB to query. This module owns
//! only the **grammar** — which frontend column key maps to which JSON field + type
//! ([`resolve`]) — and hands it to the shared, generic evaluator
//! [`trading_core::api::table_eval::apply_table_request`], which applies the request
//! (search → filters → sort → page) with the exact same op semantics as the SQL path
//! (`strategy_repo::push_filter_predicate`). Numeric operators compare numerically; a
//! numeric op on a text field is dropped just like the SQL whitelist drops it.
//!
//! Only whitelisted keys are honored (unknown → ignored). Several columns use a
//! friendlier display key than the underlying JSON field, so those are aliased here.

use chrono::TimeZone;
use serde_json::{json, Value};

use trading_core::api::table_eval::{apply_table_request, filter_table_request, resolve_token_enrichment_key, ColKind};
use trading_core::api::table_query::TableRequest;
use trading_core::strategies::kernel::{run_summary, ExitCode, RunSummary, TokenOutcome};

/// Resolve a frontend column key to the JSON field it reads + its type. `None` =
/// not filterable/sortable (dropped). Mirrors the frontend `simColumns` +
/// `appendedTokenColumns` keys — several columns use a friendlier display key
/// than the underlying JSON field name, so those are aliased here too. The
/// `appendedTokenColumns` set (`creator`, `trade_count`, `initial_buy`, `cu_limit`,
/// `migrated`, ...) reads token metadata that `token_enrich::TokenEnrichment`
/// flattens onto the row — see that module for where it's populated.
fn resolve(key: &str) -> Option<(&'static str, ColKind)> {
    use ColKind::{Number, Text};
    Some(match key {
        "mint_address" => ("mint_address", Text),
        "symbol" => ("symbol", Text),
        "reason" | "exit_reason" => ("exit_reason", Text),
        "entry_tx" => ("entry_tx", Text),
        "exit_tx" => ("exit_tx", Text),
        "target_price" => ("target_price", Number),
        "target_token_amount" => ("target_token_amount", Number),
        "target_tx" => ("target_tx", Text),
        "entry_price" => ("entry_price", Number),
        "ath_price" => ("ath_price", Number),
        "exit_price" => ("exit_price", Number),
        "entry_token_amount" => ("entry_token_amount", Number),
        "holding" | "holding_secs" => ("holding_secs", Number),
        "pnl_pct" | "pnl_percent" => ("pnl_percent", Number),
        "pnl_sol" => ("pnl_sol", Number),
        // Time fields sort/filter lexicographically on the RFC3339 string, which is
        // chronological — treat as text.
        "entry_time" => ("entry_time", Text),
        "exit_time" => ("exit_time", Text),
        "target_time" => ("target_time", Text),
        // The sim row owns its own `created_at` (the token's), so it maps `created`
        // here rather than through the shared enrichment resolver (which excludes it).
        "created" | "created_at" => ("created_at", Text),

        // --- shared token_enrich::TokenEnrichment fields (appendedTokenColumns) ---
        // Single-sourced with the live Holdings table via `resolve_token_enrichment_key`.
        _ => return resolve_token_enrichment_key(key),
    })
}

/// One page of a finished sim's rows after applying the request's search + filters +
/// sort, plus the total match count (before paging) for `X-Total-Count`. The returned
/// rows are cloned refs into the shared `Arc` payload. Thin adapter over the shared
/// [`apply_table_request`] evaluator with the sim's [`resolve`] grammar.
pub fn query(rows: &[Value], req: &TableRequest) -> (Vec<Value>, usize) {
    apply_table_request(rows, req, resolve)
}

/// Every row matching `req`'s search + filters (no sort/page) — for summary roll-ups.
pub fn filter_rows(rows: &[Value], req: &TableRequest) -> Vec<Value> {
    filter_table_request(rows, req, resolve)
}

/// Narrow one sim result row (the JSON shape [`super::replay::outcome_to_row`]
/// emits) to the kernel's [`TokenOutcome`]. Every row in a finished sim's payload
/// is an *entered* position — the replay only emits a row once an entry filled —
/// so `fired` is unconditionally true and `n_fired` is the row count.
///
/// "Closed" is decided by `exit_reason`, **not** by `exit_time != null`: the
/// analysis-only death-close (`ExitCode::Dead`) is a genuine close that carries no
/// exit tx/time, and reading `exit_time` would misfile it as open. `exit_reason` is
/// the same discriminator the sweep aggregates on, which is the point.
fn row_to_outcome(row: &Value) -> TokenOutcome {
    let num = |k: &str| -> f64 { row.get(k).and_then(Value::as_f64).unwrap_or(0.0) };
    let exit = row
        .get("exit_reason")
        .and_then(Value::as_str)
        .map(ExitCode::from_reason)
        .unwrap_or(ExitCode::Open);
    TokenOutcome {
        fired: true,
        // Open rows carry `holding_secs: null`; the kernel excludes them from every
        // holding statistic anyway, so 0 is never summed.
        holding_secs: row.get("holding_secs").and_then(Value::as_i64).unwrap_or(0),
        pnl_percent: num("pnl_percent") as f32,
        pnl_sol: num("pnl_sol") as f32,
        exit,
    }
}

/// Aggregate rollup over a finished sim's rows, shared by the filtered
/// Simulated-summary card
/// ([`super::super::api::handlers::strategies::positions::sim_result_summary`])
/// and the unfiltered rules-table last-simulation rollup ([`super::sim_spawn`]).
///
/// **Delegates to the core kernel** ([`run_summary`]) rather than counting here,
/// so a single-rule simulate and a grouped-sweep combo over the same outcomes
/// produce byte-identical numbers — same realized-only semantics in the
/// `realized` band (a still-`Open` mark feeds `n_fired`/`n_open`/`open_pnl_sol`
/// and nothing else), same win-rate denominator, same exit-code buckets, and the
/// same `mtm` counterpart band. The previous hand-rolled version summed open
/// marks into a single `total_pnl_sol` and averaged `pnl_percent` over open rows
/// too, so a rule holding its losers open read as profitable here while the sweep
/// reported the loss (parity plan B1-B4).
///
/// Exact (not sketch) quantiles: a sim's row set is bounded — one rule over one
/// corpus, already resident in RAM — so this matches the sweep **drill-in**
/// (`ComboMetrics::exact_from_rows`) precisely. The persisted sweep row goes
/// through the streaming DDSketch instead and carries ~15% error on the two
/// interior quantiles; that is a property of the unbounded combos × tokens fold,
/// not a parity break here.
pub fn summarize(rows: &[Value]) -> RunSummary {
    let outcomes: Vec<TokenOutcome> = rows.iter().map(row_to_outcome).collect();
    run_summary(outcomes.iter())
}

// ── Temporal summary (hold bins + wall-clock heatmap) ─────────────────────────
//
// Wire shape mirrors `frontend/.../temporalSummary.ts` (`TemporalSummaryData`).
// Hold-bin **scheme** adapts to cohort density (p90 of closed holding_secs) —
// twin of FE `pickHoldScheme` / `holdBinsFor`. Edges are integer seconds
// (filters use `15..59` etc.).

/// Wall-clock field the heatmap bins on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallTimeField {
    EntryTime,
    CreatedAt,
}

impl WallTimeField {
    pub fn parse(s: &str) -> Self {
        match s {
            "created_at" => WallTimeField::CreatedAt,
            _ => WallTimeField::EntryTime,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            WallTimeField::EntryTime => "entry_time",
            WallTimeField::CreatedAt => "created_at",
        }
    }

    fn json_key(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Clone, Copy)]
struct HoldBinDef {
    id: &'static str,
    label: &'static str,
    lo: Option<i64>,
    hi: Option<i64>,
    is_open: bool,
}

/// Adaptive hold-duration scale — twin of FE `HoldScheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldScheme {
    Dense15s,
    Dense60s,
    Mid5m,
    Mid30m,
    Wide2h,
    WideDay,
}

impl HoldScheme {
    fn as_str(self) -> &'static str {
        match self {
            HoldScheme::Dense15s => "dense_15s",
            HoldScheme::Dense60s => "dense_60s",
            HoldScheme::Mid5m => "mid_5m",
            HoldScheme::Mid30m => "mid_30m",
            HoldScheme::Wide2h => "wide_2h",
            HoldScheme::WideDay => "wide_day",
        }
    }

    fn parse_override(s: &str) -> Option<Self> {
        match s {
            "dense_15s" => Some(HoldScheme::Dense15s),
            "dense_60s" => Some(HoldScheme::Dense60s),
            "mid_5m" => Some(HoldScheme::Mid5m),
            "mid_30m" => Some(HoldScheme::Mid30m),
            "wide_2h" => Some(HoldScheme::Wide2h),
            "wide_day" => Some(HoldScheme::WideDay),
            _ => None,
        }
    }
}

const OPEN_HOLD_BIN: HoldBinDef = HoldBinDef {
    id: "open",
    label: "Open",
    lo: None,
    hi: None,
    is_open: true,
};

/// Inclusive integer-second bins per scheme — twin of FE `HOLD_SCHEME_EDGES`.
fn hold_bins_for(scheme: HoldScheme) -> &'static [HoldBinDef] {
    match scheme {
        HoldScheme::Dense15s => &[
            HoldBinDef { id: "hold_0_2", label: "<3s", lo: Some(0), hi: Some(2), is_open: false },
            HoldBinDef { id: "hold_3_5", label: "3–6s", lo: Some(3), hi: Some(5), is_open: false },
            HoldBinDef { id: "hold_6_9", label: "6–10s", lo: Some(6), hi: Some(9), is_open: false },
            HoldBinDef { id: "hold_10_14", label: "10–15s", lo: Some(10), hi: Some(14), is_open: false },
            HoldBinDef { id: "hold_15_plus", label: "15s+", lo: Some(15), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
        HoldScheme::Dense60s => &[
            HoldBinDef { id: "hold_0_9", label: "<10s", lo: Some(0), hi: Some(9), is_open: false },
            HoldBinDef { id: "hold_10_19", label: "10–20s", lo: Some(10), hi: Some(19), is_open: false },
            HoldBinDef { id: "hold_20_39", label: "20–40s", lo: Some(20), hi: Some(39), is_open: false },
            HoldBinDef { id: "hold_40_59", label: "40–60s", lo: Some(40), hi: Some(59), is_open: false },
            HoldBinDef { id: "hold_60_plus", label: "60s+", lo: Some(60), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
        HoldScheme::Mid5m => &[
            HoldBinDef { id: "hold_0_29", label: "<30s", lo: Some(0), hi: Some(29), is_open: false },
            HoldBinDef { id: "hold_30_59", label: "30–60s", lo: Some(30), hi: Some(59), is_open: false },
            HoldBinDef { id: "hold_60_119", label: "1–2m", lo: Some(60), hi: Some(119), is_open: false },
            HoldBinDef { id: "hold_120_299", label: "2–5m", lo: Some(120), hi: Some(299), is_open: false },
            HoldBinDef { id: "hold_300_plus", label: "5m+", lo: Some(300), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
        HoldScheme::Mid30m => &[
            HoldBinDef { id: "hold_0_14", label: "<15s", lo: Some(0), hi: Some(14), is_open: false },
            HoldBinDef { id: "hold_15_59", label: "15–60s", lo: Some(15), hi: Some(59), is_open: false },
            HoldBinDef { id: "hold_60_299", label: "1–5m", lo: Some(60), hi: Some(299), is_open: false },
            HoldBinDef { id: "hold_300_1799", label: "5–30m", lo: Some(300), hi: Some(1799), is_open: false },
            HoldBinDef { id: "hold_1800_plus", label: "30m+", lo: Some(1800), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
        HoldScheme::Wide2h => &[
            HoldBinDef { id: "hold_0_59", label: "<1m", lo: Some(0), hi: Some(59), is_open: false },
            HoldBinDef { id: "hold_60_299", label: "1–5m", lo: Some(60), hi: Some(299), is_open: false },
            HoldBinDef { id: "hold_300_899", label: "5–15m", lo: Some(300), hi: Some(899), is_open: false },
            HoldBinDef { id: "hold_900_3599", label: "15–60m", lo: Some(900), hi: Some(3599), is_open: false },
            HoldBinDef { id: "hold_3600_plus", label: "1h+", lo: Some(3600), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
        HoldScheme::WideDay => &[
            HoldBinDef { id: "hold_0_299", label: "<5m", lo: Some(0), hi: Some(299), is_open: false },
            HoldBinDef { id: "hold_300_1799", label: "5–30m", lo: Some(300), hi: Some(1799), is_open: false },
            HoldBinDef { id: "hold_1800_7199", label: "30m–2h", lo: Some(1800), hi: Some(7199), is_open: false },
            HoldBinDef { id: "hold_7200_21599", label: "2–6h", lo: Some(7200), hi: Some(21_599), is_open: false },
            HoldBinDef { id: "hold_21600_plus", label: "6h+", lo: Some(21_600), hi: None, is_open: false },
            OPEN_HOLD_BIN,
        ],
    }
}

/// p90 of closed holding_secs → scheme. Twin of FE `pickHoldScheme`.
fn pick_hold_scheme(closed_secs: &[i64]) -> HoldScheme {
    if closed_secs.is_empty() {
        return HoldScheme::Mid30m;
    }
    let mut sorted: Vec<i64> = closed_secs.iter().copied().filter(|s| *s >= 0).collect();
    if sorted.is_empty() {
        return HoldScheme::Mid30m;
    }
    sorted.sort_unstable();
    // Nearest-rank p90 (`ceil(0.9·n)−1`) — twin of FE `pickHoldScheme`.
    let idx = ((0.9 * sorted.len() as f64).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    let p90 = sorted[idx];
    if p90 <= 15 {
        HoldScheme::Dense15s
    } else if p90 <= 60 {
        HoldScheme::Dense60s
    } else if p90 <= 300 {
        HoldScheme::Mid5m
    } else if p90 <= 1800 {
        HoldScheme::Mid30m
    } else if p90 <= 7200 {
        HoldScheme::Wide2h
    } else {
        HoldScheme::WideDay
    }
}

fn holding_filter_for(b: &HoldBinDef) -> Option<String> {
    if b.is_open {
        return None;
    }
    let lo = b.lo?;
    match b.hi {
        Some(hi) => Some(format!("{lo}..{hi}")),
        None => Some(format!(">={lo}")),
    }
}

fn empty_exits() -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    for k in [
        "n_exit_take_profit",
        "n_exit_stop_loss",
        "n_exit_metrics",
        "n_exit_dead",
        "n_exit_manual",
        "n_exit_trailing",
        "n_exit_stall",
        "n_exit_time",
        "n_exit_liquidity",
        "n_exit_next_kill",
        "other",
        "open",
    ] {
        m.insert(k.into(), json!(0));
    }
    m
}

fn exit_key(code: ExitCode) -> &'static str {
    match code {
        ExitCode::TakeProfit => "n_exit_take_profit",
        ExitCode::StopLoss => "n_exit_stop_loss",
        ExitCode::Metrics => "n_exit_metrics",
        ExitCode::Dead => "n_exit_dead",
        ExitCode::TrailingStop => "n_exit_trailing",
        ExitCode::Stall => "n_exit_stall",
        ExitCode::TimeStop => "n_exit_time",
        ExitCode::LiquidityExit => "n_exit_liquidity",
        ExitCode::NextKill => "n_exit_next_kill",
        ExitCode::Open | ExitCode::NoEntry => "open",
    }
}

fn tally_exit(exits: &mut serde_json::Map<String, Value>, reason: &str) {
    let key = if reason == "Manual" {
        "n_exit_manual"
    } else if reason == "Open" {
        "open"
    } else {
        exit_key(ExitCode::from_reason(reason))
    };
    let n = exits.get(key).and_then(Value::as_i64).unwrap_or(0) + 1;
    exits.insert(key.into(), json!(n));
}

fn hold_bin_id(exit: &str, holding_secs: i64, bins: &[HoldBinDef]) -> &'static str {
    if exit == "Open" {
        return "open";
    }
    if holding_secs < 0 {
        return "open";
    }
    for b in bins {
        if b.is_open {
            continue;
        }
        let Some(lo) = b.lo else { continue };
        if holding_secs < lo {
            continue;
        }
        if let Some(hi) = b.hi {
            if holding_secs > hi {
                continue;
            }
        }
        return b.id;
    }
    bins.iter()
        .rev()
        .find(|b| !b.is_open)
        .map(|b| b.id)
        .unwrap_or("open")
}

fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    // Accept RFC3339 / ISO-8601; chrono via DateTime parse used elsewhere in lab —
    // keep this light: use `time`/`chrono` if available on trading_core rows.
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .or_else(|| {
            // Some rows may omit the offset suffix; try appending Z.
            if !s.ends_with('Z') && !s.contains('+') {
                chrono::DateTime::parse_from_rfc3339(&format!("{s}Z"))
                    .ok()
                    .map(|dt| dt.timestamp_millis())
            } else {
                None
            }
        })
}

/// Adaptive wall-clock bucket — twin of FE `WallGrain` / `pickWallGrain`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WallGrain {
    M30,
    H1,
    H2,
    H4,
    Day,
}

impl WallGrain {
    fn as_str(self) -> &'static str {
        match self {
            WallGrain::M30 => "30m",
            WallGrain::H1 => "1h",
            WallGrain::H2 => "2h",
            WallGrain::H4 => "4h",
            WallGrain::Day => "day",
        }
    }

    fn step_ms(self) -> i64 {
        match self {
            WallGrain::M30 => 30 * 60_000,
            WallGrain::H1 => 3_600_000,
            WallGrain::H2 => 2 * 3_600_000,
            WallGrain::H4 => 4 * 3_600_000,
            WallGrain::Day => 86_400_000,
        }
    }

    /// `None` = auto (adaptive pick). Unknown strings also mean auto.
    fn parse_override(s: &str) -> Option<Self> {
        match s {
            "30m" => Some(WallGrain::M30),
            "1h" => Some(WallGrain::H1),
            "2h" => Some(WallGrain::H2),
            "4h" => Some(WallGrain::H4),
            "day" => Some(WallGrain::Day),
            _ => None,
        }
    }
}

fn pick_wall_grain(span_ms: i64) -> WallGrain {
    const H: i64 = 3_600_000;
    const D: i64 = 86_400_000;
    let span = span_ms.max(0);
    if span <= 6 * H {
        WallGrain::M30
    } else if span <= 24 * H {
        WallGrain::H1
    } else if span <= 3 * D {
        WallGrain::H2
    } else if span <= 7 * D {
        WallGrain::H4
    } else {
        WallGrain::Day
    }
}

fn floor_to_grain(ms: i64, grain: WallGrain) -> i64 {
    let step = grain.step_ms();
    ms - ms.rem_euclid(step)
}

fn dominant_exit(exits: &serde_json::Map<String, Value>) -> Option<&'static str> {
    const ORDER: &[(&str, &str)] = &[
        ("n_exit_take_profit", "Take profit"),
        ("n_exit_stop_loss", "Stop loss"),
        ("n_exit_metrics", "Metric"),
        ("n_exit_dead", "Dead"),
        ("n_exit_manual", "Manual"),
        ("n_exit_trailing", "Trailing"),
        ("n_exit_stall", "Stall"),
        ("n_exit_time", "Time"),
        ("n_exit_liquidity", "Liquidity"),
        ("n_exit_next_kill", "Next kill"),
        ("open", "Open"),
        ("other", "Other"),
    ];
    let mut best: Option<&'static str> = None;
    let mut n = 0i64;
    for &(k, label) in ORDER {
        let c = exits.get(k).and_then(Value::as_i64).unwrap_or(0);
        if c > n {
            n = c;
            best = Some(label);
        }
    }
    best
}

/// Fold filtered sim rows into the temporal summary payload the FE timeline renders.
/// `grain_override` / `hold_scheme_override`: `None` / auto → adaptive pick; `Some` forces.
pub fn time_summary(
    rows: &[Value],
    wall_field: WallTimeField,
    grain_override: Option<&str>,
    hold_scheme_override: Option<&str>,
) -> Value {
    let closed_secs: Vec<i64> = rows
        .iter()
        .filter(|r| r.get("exit_reason").and_then(Value::as_str).unwrap_or("Open") != "Open")
        .map(|r| r.get("holding_secs").and_then(Value::as_i64).unwrap_or(0))
        .filter(|s| *s >= 0)
        .collect();
    let hold_scheme_auto = pick_hold_scheme(&closed_secs);
    let hold_scheme = hold_scheme_override
        .and_then(HoldScheme::parse_override)
        .unwrap_or(hold_scheme_auto);
    let bins = hold_bins_for(hold_scheme);

    let mut hold: Vec<Value> = bins
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "label": b.label,
                "n": 0,
                "pnl_sol": 0.0,
                "exits": empty_exits(),
                "holdingFilter": holding_filter_for(b),
                "exitFilter": if b.is_open { Value::String("Open".into()) } else { Value::Null },
                "mints": [],
            })
        })
        .collect();
    let hold_index: std::collections::HashMap<&str, usize> = bins
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();

    let mut times: Vec<i64> = Vec::new();
    let mut n_fired = 0usize;
    let field_key = wall_field.json_key();

    for row in rows {
        n_fired += 1; // every sim row is an entered position
        let mint = row
            .get("mint_address")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let exit = row
            .get("exit_reason")
            .and_then(Value::as_str)
            .unwrap_or("Open");
        let holding = row.get("holding_secs").and_then(Value::as_i64).unwrap_or(0);
        let pnl = row.get("pnl_sol").and_then(Value::as_f64).unwrap_or(0.0);
        let bin_id = hold_bin_id(exit, holding, bins);
        if let Some(&idx) = hold_index.get(bin_id) {
            let obj = hold[idx].as_object_mut().expect("hold bin object");
            let n = obj.get("n").and_then(Value::as_i64).unwrap_or(0) + 1;
            obj.insert("n".into(), json!(n));
            let p = obj.get("pnl_sol").and_then(Value::as_f64).unwrap_or(0.0) + pnl;
            obj.insert("pnl_sol".into(), json!(p));
            if let Some(Value::Object(ex)) = obj.get_mut("exits") {
                tally_exit(ex, exit);
            }
            if !mint.is_empty() {
                if let Some(Value::Array(mints)) = obj.get_mut("mints") {
                    mints.push(Value::String(mint.clone()));
                }
            }
        }
        if let Some(ts) = row
            .get(field_key)
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
        {
            times.push(ts);
        }
    }

    let forced = grain_override.and_then(WallGrain::parse_override);

    let (wall_grain, wall_grain_auto, wall_span_ms, wall_cells) = if times.is_empty() {
        (WallGrain::Day, WallGrain::Day, 0i64, Vec::new())
    } else {
        let min_t = *times.iter().min().unwrap();
        let max_t = *times.iter().max().unwrap();
        let span = (max_t - min_t).max(0);
        let auto = pick_wall_grain(span);
        let grain = forced.unwrap_or(auto);
        let step = grain.step_ms();
        let start0 = floor_to_grain(min_t, grain);
        let end0 = floor_to_grain(max_t, grain) + step;
        let mut cells: std::collections::BTreeMap<
            i64,
            (i64, f64, serde_json::Map<String, Value>, i64, Vec<String>),
        > = std::collections::BTreeMap::new();
        let mut t = start0;
        while t < end0 {
            cells.insert(t, (0, 0.0, empty_exits(), 0, Vec::new()));
            t += step;
        }
        for row in rows {
            let mint = row
                .get("mint_address")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let exit = row
                .get("exit_reason")
                .and_then(Value::as_str)
                .unwrap_or("Open");
            let pnl = row.get("pnl_sol").and_then(Value::as_f64).unwrap_or(0.0);
            let Some(ts) = row
                .get(field_key)
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_ms)
            else {
                continue;
            };
            let key = floor_to_grain(ts, grain);
            if let Some(cell) = cells.get_mut(&key) {
                cell.0 += 1;
                cell.1 += pnl;
                tally_exit(&mut cell.2, exit);
                if exit != "Open" && pnl > 0.0 {
                    cell.3 += 1;
                }
                if !mint.is_empty() {
                    cell.4.push(mint);
                }
            }
        }
        let wall: Vec<Value> = cells
            .into_iter()
            .map(|(key, (n, pnl, exits, wins, mints))| {
                let closed = n - exits.get("open").and_then(Value::as_i64).unwrap_or(0);
                let win_rate = if closed > 0 {
                    wins as f64 / closed as f64
                } else {
                    0.0
                };
                let dominant = if n > 0 {
                    dominant_exit(&exits).map(Value::from).unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                json!({
                    "id": format!("{}:{key}", wall_field.as_str()),
                    "start": chrono::Utc
                        .timestamp_millis_opt(key)
                        .single()
                        .map(|d| d.to_rfc3339())
                        .unwrap_or_default(),
                    "end": chrono::Utc
                        .timestamp_millis_opt(key + step)
                        .single()
                        .map(|d| d.to_rfc3339())
                        .unwrap_or_default(),
                    "n": n,
                    "pnl_sol": pnl,
                    "win_rate": win_rate,
                    "exits": exits,
                    "dominant": dominant,
                    "mints": mints,
                })
            })
            .collect();
        (grain, auto, span, wall)
    };

    json!({
        "hold": hold,
        "holdScheme": hold_scheme.as_str(),
        "holdSchemeAuto": hold_scheme_auto.as_str(),
        "wall": wall_cells,
        "wallGrain": wall_grain.as_str(),
        "wallGrainAuto": wall_grain_auto.as_str(),
        "wallSpanMs": wall_span_ms,
        "wallField": wall_field.as_str(),
        "nFired": n_fired,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({"mint_address":"a","symbol":"BONK","pnl_percent":10.0,"pnl_sol":1.0,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint_address":"b","symbol":"pumpcat","pnl_percent":-5.0,"pnl_sol":-0.5,"exit_time":"2026-01-01T00:00:00Z"}),
            json!({"mint_address":"c","symbol":"WIF","pnl_percent":50.0,"pnl_sol":2.0,"exit_time":null}),
        ]
    }

    fn req(json: serde_json::Value) -> TableRequest {
        serde_json::from_value(json).expect("TableRequest")
    }

    // ── summary parity (plan B1-B4) ─────────────────────────────────────────

    /// A sim row as `outcome_to_row` emits it: `exit_reason` always present,
    /// `exit_time`/`holding_secs` null on a still-open position.
    fn sim_row(pnl_sol: f64, pnl_pct: f64, exit: &str, holding: Option<i64>) -> Value {
        json!({
            "mint_address": "m", "symbol": "S",
            "pnl_sol": pnl_sol, "pnl_percent": pnl_pct,
            "exit_reason": exit,
            "holding_secs": holding,
            "exit_time": if exit == "Open" { Value::Null } else { json!("2026-01-01T00:00:00Z") },
        })
    }

    #[test]
    fn open_marks_are_excluded_from_the_headline_total() {
        // The regression this whole change exists for: a rule whose losers closed
        // and whose big winner is still open must NOT report the open mark in
        // `total_pnl_sol`. The old hand-rolled rollup summed every row.
        let rows = vec![
            sim_row(1.0, 50.0, "TakeProfit", Some(10)),
            sim_row(-1.0, -50.0, "StopLoss", Some(10)),
            sim_row(1_000.0, 5_000.0, "Open", None),
        ];
        let m = summarize(&rows).realized;
        assert_eq!(m.n_fired, 3);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 2);
        assert!((m.total_pnl_sol - 0.0).abs() < 1e-9, "realized total excludes the open mark");
        assert!((m.open_pnl_sol - 1_000.0).abs() < 1e-9, "open mark surfaced separately");
        assert!((m.win_rate - 0.5).abs() < 1e-9, "win rate over closed only");
        assert_eq!(m.best_pnl_pct, 50.0, "the open mark must not become the best");
    }

    #[test]
    fn death_close_counts_as_closed_despite_a_null_exit_time() {
        // `ExitCode::Dead` is a genuine close that carries no exit tx/time. The old
        // rollup keyed "closed" off `exit_time != null` and so booked it as open.
        let rows = vec![sim_row(-0.4, -80.0, "Dead", Some(600))];
        let m = summarize(&rows).realized;
        assert_eq!(m.n_closed, 1, "a death-close is closed");
        assert_eq!(m.n_open, 0);
        assert_eq!(m.n_exit_dead, 1);
        assert!((m.total_pnl_sol - -0.4).abs() < 1e-6, "its loss lands in the realized total");
    }

    #[test]
    fn simulate_summary_equals_the_sweep_drill_in_on_the_same_outcomes() {
        // The parity lock: identical outcomes must roll up identically through the
        // simulate path (JSON rows → `summarize`) and the grouped-sweep drill-in
        // (`ComboTokenResult` rows → `ComboMetrics::exact_from_rows`). Both now
        // delegate to `exact_run_metrics`, so this can only break if one of them
        // grows a private aggregate again.
        use crate::sweep::aggregate::ComboMetrics;
        use trading_core::models::grouped_sweep::ComboTokenResult;

        let specs: Vec<(f64, f64, &str, Option<i64>)> = vec![
            (2.0, 100.0, "TakeProfit", Some(10)),
            (-1.0, -50.0, "StopLoss", Some(20)),
            (0.5, 25.0, "Metrics", Some(35)),
            (-0.4, -80.0, "Dead", Some(600)),
            (5.0, 999.0, "Open", None),
        ];

        let sim_rows: Vec<Value> =
            specs.iter().map(|&(sol, pct, ex, h)| sim_row(sol, pct, ex, h)).collect();
        let sweep_rows: Vec<ComboTokenResult> = specs
            .iter()
            .map(|&(sol, pct, ex, h)| ComboTokenResult {
                mint_address: "m".into(),
                symbol: "S".into(),
                fired: true,
                pnl_sol: sol as f32,
                pnl_pct: pct as f32,
                holding_secs: h.unwrap_or(0),
                exit: ex.into(),
                entry_time: None,
                entry_price: None,
                entry_tx: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_tx: None,
                exit_slot: None,
                created_at: None,
                ath_price: None,
                token: Default::default(),
            })
            .collect();

        let sim = summarize(&sim_rows).realized;
        let sweep = ComboMetrics::exact_from_rows(0, &sweep_rows);

        assert_eq!(sim.n_fired, sweep.n_fired);
        assert_eq!(sim.n_open, sweep.n_open);
        assert_eq!(sim.n_closed, sweep.n_closed);
        assert!((sim.win_rate - sweep.win_rate).abs() < 1e-9);
        assert!((sim.total_pnl_sol - sweep.total_pnl_sol).abs() < 1e-6);
        assert!((sim.open_pnl_sol - sweep.open_pnl_sol).abs() < 1e-6);
        assert!((sim.mean_pnl_pct - sweep.mean_pnl_pct).abs() < 1e-6);
        assert!((sim.median_pnl_pct - sweep.median_pnl_pct).abs() < 1e-6);
        assert!((sim.expectancy_sol - sweep.expectancy_sol).abs() < 1e-6);
        assert!((sim.avg_holding_secs - sweep.avg_holding_secs).abs() < 1e-9);
        assert_eq!(sim.n_exit_dead, sweep.n_exit_dead);
        assert_eq!(sim.n_exit_metrics, sweep.n_exit_metrics);
        assert_eq!(sim.profit_factor.is_some(), sweep.profit_factor.is_some());
    }

    #[test]
    fn time_summary_hold_bins_match_edges() {
        let rows = vec![
            json!({
                "mint_address":"a","exit_reason":"TakeProfit","holding_secs":10,
                "pnl_sol":1.0,"entry_time":"2026-07-15T14:30:00Z"
            }),
            json!({
                "mint_address":"b","exit_reason":"StopLoss","holding_secs":20,
                "pnl_sol":-0.5,"entry_time":"2026-07-15T14:45:00Z"
            }),
            json!({
                "mint_address":"c","exit_reason":"Open","holding_secs":0,
                "pnl_sol":0.1,"entry_time":"2026-07-15T15:00:00Z"
            }),
        ];
        let body = time_summary(&rows, WallTimeField::EntryTime, None, None);
        assert_eq!(body["nFired"], 3);
        // Closed holds 10s + 20s → p90=20 → dense_60s
        assert_eq!(body["holdScheme"], "dense_60s");
        assert_eq!(body["holdSchemeAuto"], "dense_60s");
        let hold = body["hold"].as_array().unwrap();
        let b10 = hold.iter().find(|b| b["id"] == "hold_10_19").unwrap();
        assert_eq!(b10["n"], 1);
        assert_eq!(b10["exits"]["n_exit_take_profit"], 1);
        let b20 = hold.iter().find(|b| b["id"] == "hold_20_39").unwrap();
        assert_eq!(b20["n"], 1);
        let open = hold.iter().find(|b| b["id"] == "open").unwrap();
        assert_eq!(open["n"], 1);
        assert_eq!(body["wallGrain"], "30m");
        assert_eq!(body["wallGrainAuto"], "30m");
        assert!(body["wallSpanMs"].as_i64().unwrap() > 0);
        let wall_n: i64 = body["wall"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["n"].as_i64().unwrap_or(0))
            .sum();
        assert_eq!(wall_n, 3);

        let forced = time_summary(&rows, WallTimeField::EntryTime, Some("1h"), Some("dense_15s"));
        assert_eq!(forced["wallGrain"], "1h");
        assert_eq!(forced["wallGrainAuto"], "30m");
        assert_eq!(forced["holdScheme"], "dense_15s");
        assert_eq!(forced["holdSchemeAuto"], "dense_60s");
    }

    #[test]
    fn pick_hold_scheme_tracks_density() {
        assert_eq!(pick_hold_scheme(&[5, 8, 12]), HoldScheme::Dense15s);
        assert_eq!(pick_hold_scheme(&[10, 20, 45]), HoldScheme::Dense60s);
        assert_eq!(pick_hold_scheme(&[60, 120, 240]), HoldScheme::Mid5m);
        assert_eq!(pick_hold_scheme(&[300, 600, 1200]), HoldScheme::Mid30m);
        assert_eq!(pick_hold_scheme(&[3600, 4000, 5000]), HoldScheme::Wide2h);
        assert_eq!(pick_hold_scheme(&[10_000, 20_000]), HoldScheme::WideDay);
        assert_eq!(pick_hold_scheme(&[]), HoldScheme::Mid30m);
    }

    #[test]
    fn pick_wall_grain_matches_fe_thresholds() {
        const H: i64 = 3_600_000;
        const D: i64 = 86_400_000;
        assert_eq!(pick_wall_grain(2 * H), WallGrain::M30);
        assert_eq!(pick_wall_grain(12 * H), WallGrain::H1);
        assert_eq!(pick_wall_grain(2 * D), WallGrain::H2);
        assert_eq!(pick_wall_grain(5 * D), WallGrain::H4);
        assert_eq!(pick_wall_grain(14 * D), WallGrain::Day);
    }

    #[test]
    fn filter_rows_matches_query_total_without_paging() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows(), &r);
        let filtered = filter_rows(&rows(), &r);
        assert_eq!(filtered.len(), total, "filter_rows count must match query total");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn numeric_gt_filters_server_side() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"gt","val":5}}}));
        let (page, total) = query(&rows(), &r);
        assert_eq!(total, 2, "pnl_percent > 5 keeps BONK(10) + WIF(50)");
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn between_is_inclusive() {
        let r = req(json!({"filters": {"pnl_percent": {"op":"between","min":10,"max":50}}}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 2, "10..=50 keeps 10 and 50");
    }

    #[test]
    fn numeric_op_on_text_col_is_ignored() {
        // `symbol` is Text; a numeric `gt` must not constrain (mirrors SQL drop).
        let r = req(json!({"filters": {"symbol": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 3, "numeric op on text col keeps all rows");
    }

    #[test]
    fn search_matches_symbol_substring() {
        let r = req(json!({"search": "pump"}));
        let (_, total) = query(&rows(), &r);
        assert_eq!(total, 1, "search 'pump' matches only pumpcat");
    }

    #[test]
    fn frontend_display_key_aliases_resolve() {
        // Frontend `simColumns` keys (`holding`, `pnl_pct`, `reason`) are friendlier
        // than the backend field names — must resolve identically.
        let rows = vec![
            json!({"mint_address":"a","symbol":"BONK","trade_count":3,"exit_reason":"TakeProfit"}),
            json!({"mint_address":"b","symbol":"WIF","trade_count":9,"exit_reason":"StopLoss"}),
        ];
        let r = req(json!({"filters": {"trade_count": {"op":"gt","val":5}}}));
        let (_, total) = query(&rows, &r);
        assert_eq!(total, 1, "'trade_count' (Token Trades) filters the enrichment count");

        let r = req(json!({"filters": {"reason": {"op":"eq","val":"StopLoss"}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'reason' alias must filter on exit_reason");
        assert_eq!(page[0]["mint_address"], "b");

        let r = req(json!({"sorting": [{"col":"holding","dir":"asc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page.len(), 2, "'holding' alias must not drop rows lacking holding_secs");
    }

    #[test]
    fn token_enrichment_fields_sort_and_filter() {
        // Fields flattened onto the row by `token_enrich::TokenEnrichment` — the
        // frontend's `appendedTokenColumns` display keys must alias to them.
        let rows = vec![
            json!({"mint_address":"a","symbol":"BONK","initial_buy_sol":0.5,"cu_price":1000,"is_migrated":true}),
            json!({"mint_address":"b","symbol":"WIF","initial_buy_sol":2.0,"cu_price":5000,"is_migrated":false}),
        ];

        let r = req(json!({"filters": {"initial_buy": {"op":"gt","val":1.0}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 1, "'initial_buy' alias must filter on initial_buy_sol");
        assert_eq!(page[0]["mint_address"], "b");

        let r = req(json!({"sorting": [{"col":"cu_price","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint_address"], "b", "cu_price sorts desc: WIF(5000) first");

        // Booleans coerce to 0.0/1.0 for sort — true (migrated) sorts before false.
        let r = req(json!({"sorting": [{"col":"migrated","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(page[0]["mint_address"], "a", "'migrated' alias sorts is_migrated true first (desc)");
    }

    #[test]
    fn sort_desc_and_page() {
        let r = req(json!({
            "sorting": [{"col":"pnl_percent","dir":"desc"}],
            "pagination": {"page":1,"pageSize":2}
        }));
        let (page, total) = query(&rows(), &r);
        assert_eq!(total, 3);
        assert_eq!(page.len(), 2, "page size 2");
        assert_eq!(page[0]["mint_address"], "c", "WIF(50) first desc");
        assert_eq!(page[1]["mint_address"], "a", "BONK(10) second");
    }

    #[test]
    fn equal_sort_key_breaks_ties_by_mint_asc() {
        // Three rows share pnl_sol=1.0 — without a tiebreak their order is unstable
        // across page seams. The `mint` ASC tail must pin them to b < m < z.
        let rows = vec![
            json!({"mint_address":"z","symbol":"Z","pnl_sol":1.0}),
            json!({"mint_address":"b","symbol":"B","pnl_sol":1.0}),
            json!({"mint_address":"m","symbol":"M","pnl_sol":1.0}),
        ];
        let r = req(json!({"sorting": [{"col":"pnl_sol","dir":"desc"}]}));
        let (page, _) = query(&rows, &r);
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["b", "m", "z"],
            "equal pnl_sol rows order by mint ASC"
        );
    }

    #[test]
    fn no_sort_column_still_orders_by_mint() {
        // Default view (no sort levels) must still be deterministic → mint ASC.
        let rows = vec![
            json!({"mint_address":"z","symbol":"Z"}),
            json!({"mint_address":"a","symbol":"A"}),
            json!({"mint_address":"m","symbol":"M"}),
        ];
        let (page, _) = query(&rows, &req(json!({})));
        assert_eq!(
            page.iter().map(|r| r["mint_address"].as_str().unwrap()).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "no sort key → stable mint ASC order"
        );
    }
}
