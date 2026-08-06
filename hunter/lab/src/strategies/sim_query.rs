//! In-memory server-side query over a finished backtest's per-token results.
//!
//! The Simulated token table pages/sorts/filters/searches over the **unified**
//! `TableRequest` contract — same shape as Positions/Matched — but its data source
//! is the working-set `Vec<Value>` hydrated from [`SimResults`](crate::state::sim_results)
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
        // Bool JSON → 0/1 via `field_num` (same as the Fired Yes/No column).
        "fired" => ("fired", Number),
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

/// Narrow one sim result row (the JSON shape [`super::replay::outcome_to_row`] /
/// [`super::replay::no_entry_row`] emit) to the kernel's [`TokenOutcome`].
///
/// Rows cover the full matched candidate set: entered positions (`fired: true`)
/// and matched-but-never-entered (`exit_reason: "NoEntry"`, `fired: false`). The
/// kernel skips `!fired` the same way the sweep drill-in does, so `n_fired` stays
/// the entered count.
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
    // Prefer the explicit `fired` flag (new rows); fall back to exit_reason for
    // any legacy resident payload that predated the field.
    let fired = row
        .get("fired")
        .and_then(Value::as_bool)
        .unwrap_or(exit != ExitCode::NoEntry);
    TokenOutcome {
        fired,
        // Open / NoEntry rows carry `holding_secs: null`; the kernel excludes them
        // from every holding statistic anyway, so 0 is never summed.
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

/// Whether a row is an entered position, not a matched-but-never-entered
/// `NoEntry` pad. Prefers the explicit `fired` flag (new rows); falls back to
/// `exit_reason` for any legacy resident payload that predated the field — the
/// same fallback [`row_to_outcome`] uses, kept as one function so the two never
/// drift onto different legacy heuristics.
pub fn row_is_fired(row: &Value) -> bool {
    row.get("fired")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| row.get("exit_reason").and_then(Value::as_str) != Some("NoEntry"))
}

/// Distinct-token count over a sim's rows — the true "matched candidate pool"
/// size, **not** `rows.len()`.
///
/// Rows are one per **position**, not per token: a re-entry rule
/// (`RuleParams.reentry`) can re-arm and take several entries on the same mint,
/// so [`super::replay::run_replay`] emits one [`super::replay::PositionOutcome`]
/// (→ one row) per episode. `run_engine_backtest` then pads in exactly one
/// `NoEntry` row per never-entered candidate. So every distinct `mint_address`
/// among the rows is exactly one matched token — fired once, fired N times (N
/// rows, same mint), or never fired (one `NoEntry` row) — and counting rows
/// directly would overcount any mint with more than one episode.
pub fn count_matched(rows: &[Value]) -> usize {
    rows.iter()
        .filter_map(|r| r.get("mint_address").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// Distinct-token count over a sim's **entered** rows only — how many unique
/// mints the rule actually traded, as opposed to `count_matched` (the whole
/// candidate pool, entered or not) or `n_fired` (every entry, so a re-entry
/// rule's repeat visits to one mint each add to the count). `n_fired -
/// count_tokens_entered` is therefore the run's re-entry volume: 0 for a
/// one-shot rule, positive whenever a mint fired more than once.
pub fn count_tokens_entered(rows: &[Value]) -> usize {
    rows.iter()
        .filter(|r| row_is_fired(r))
        .filter_map(|r| r.get("mint_address").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>()
        .len()
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
    /// The **decision instant**: sold, falling back to bought while still open.
    /// The FE default, and the stamp the equity curve / calendar / heatmap bin
    /// on — so every dated chart on the page lands a position on one civil day.
    ExitTime,
    EntryTime,
    CreatedAt,
}

impl WallTimeField {
    pub fn parse(s: &str) -> Self {
        match s {
            "created_at" => WallTimeField::CreatedAt,
            "exit_time" => WallTimeField::ExitTime,
            _ => WallTimeField::EntryTime,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            WallTimeField::ExitTime => "exit_time",
            WallTimeField::EntryTime => "entry_time",
            WallTimeField::CreatedAt => "created_at",
        }
    }

    /// The row's wall-clock instant — twin of FE `wallTimeMs`. Only `ExitTime`
    /// falls back (an open position has no exit stamp but still belongs on the
    /// timeline, at the moment it was bought).
    fn time_of(self, row: &Value) -> Option<i64> {
        let at = |k: &str| row.get(k).and_then(Value::as_str).and_then(parse_rfc3339_ms);
        match self {
            WallTimeField::CreatedAt => at("created_at"),
            WallTimeField::EntryTime => at("entry_time"),
            WallTimeField::ExitTime => at("exit_time").or_else(|| at("entry_time")),
        }
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
        "n_exit_migrated",
        "n_exit_trailing",
        "n_exit_stall",
        "n_exit_time",
        "n_exit_liquidity",
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
        ExitCode::Manual => "n_exit_manual",
        ExitCode::Migrated => "n_exit_migrated",
        ExitCode::Open | ExitCode::NoEntry => "open",
    }
}

fn tally_exit(exits: &mut serde_json::Map<String, Value>, reason: &str) {
    // "Manual" / "Migrated" are `from_reason` codes of their own — no special case.
    let key = if reason == "Open" || reason == "NoEntry" {
        "open"
    } else {
        let code = ExitCode::from_reason(reason);
        // Retired / unrecognized labels (e.g. legacy `"NextKill"`) must not fold
        // into `open` via `from_reason`'s `_ => Open` fallback.
        if matches!(code, ExitCode::Open | ExitCode::NoEntry) {
            "other"
        } else {
            exit_key(code)
        }
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

/// Parse an IANA zone name, falling back to UTC on anything unknown.
/// A bad `tz` must never 500 a chart request — it degrades to the old behavior.
pub fn parse_tz(name: &str) -> chrono_tz::Tz {
    name.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC)
}

/// Floor an instant to its wall-clock bucket **in `tz`** — twin of FE
/// `floorToWallGrain`.
///
/// Epoch-aligned flooring (`ms.rem_euclid(step)`) buckets in UTC, which
/// disagrees with the calendar + dow×hour heatmap on the same page: those
/// resolve civil dates in the user's zone, so a UTC-floored `day` bar starts at
/// 19:00 the previous local day for a UTC-5 user. Half-hour zones and the 2h/4h
/// grains straddle local boundaries the same way.
///
/// The local wall-clock is floored, then mapped back to an instant. A DST gap
/// (that wall-clock never happens) or fold (it happens twice) is resolved to the
/// **earliest** valid instant, so buckets stay monotonic and half-open.
fn floor_to_grain_in_zone(ms: i64, grain: WallGrain, tz: chrono_tz::Tz) -> i64 {
    use chrono::{Offset, TimeZone};
    let step = grain.step_ms();
    let Some(utc) = chrono::Utc.timestamp_millis_opt(ms).single() else {
        return ms - ms.rem_euclid(step);
    };
    let local = utc.with_timezone(&tz);
    let off_ms = local.offset().fix().local_minus_utc() as i64 * 1_000;
    let floored_local = (ms + off_ms) - (ms + off_ms).rem_euclid(step);
    let out = floored_local - off_ms;
    // Re-resolve: crossing the boundary may have changed the offset (DST).
    let Some(after) = chrono::Utc.timestamp_millis_opt(out).single() else {
        return out;
    };
    let off_after = after
        .with_timezone(&tz)
        .offset()
        .fix()
        .local_minus_utc() as i64
        * 1_000;
    if off_after == off_ms {
        out
    } else {
        floored_local - off_after
    }
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
    tz: chrono_tz::Tz,
) -> Value {
    let closed_secs: Vec<i64> = rows
        .iter()
        .filter(|r| {
            let exit = r.get("exit_reason").and_then(Value::as_str).unwrap_or("Open");
            exit != "Open" && exit != "NoEntry"
        })
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

    for row in rows {
        let exit = row
            .get("exit_reason")
            .and_then(Value::as_str)
            .unwrap_or("Open");
        // Not-fired matched candidates ride in the Positions table but are not
        // temporal outcomes (parity with FE `buildTemporalSummary`).
        if exit == "NoEntry" {
            continue;
        }
        n_fired += 1;
        let mint = row
            .get("mint_address")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
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
        if let Some(ts) = wall_field.time_of(row) {
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
        let mut cells: std::collections::BTreeMap<
            i64,
            (i64, f64, serde_json::Map<String, Value>, i64, Vec<String>),
        > = std::collections::BTreeMap::new();
        // Civil buckets are NOT uniformly `step` apart (a DST day is 23h or 25h),
        // so seed the real row keys — boundaries by construction — and let the
        // walk only fill the empty gaps. A `t += step` grid would drift off the
        // boundaries `floor_to_grain_in_zone` produces and silently drop rows.
        for t in &times {
            cells
                .entry(floor_to_grain_in_zone(*t, grain, tz))
                .or_insert_with(|| (0, 0.0, empty_exits(), 0, Vec::new()));
        }
        let start0 = floor_to_grain_in_zone(min_t, grain, tz);
        let last = floor_to_grain_in_zone(max_t, grain, tz);
        let mut t = start0;
        while t < last {
            let snapped = floor_to_grain_in_zone(t + step, grain, tz);
            // Forced monotonic so a repeated local hour can't spin the walk.
            t = if snapped > t { snapped } else { t + step };
            if t <= last {
                cells
                    .entry(t)
                    .or_insert_with(|| (0, 0.0, empty_exits(), 0, Vec::new()));
            }
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
            let Some(ts) = wall_field.time_of(row) else {
                continue;
            };
            let key = floor_to_grain_in_zone(ts, grain, tz);
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
        let ordered: Vec<i64> = cells.keys().copied().collect();
        let wall: Vec<Value> = cells
            .into_iter()
            .enumerate()
            .map(|(i, (key, (n, pnl, exits, wins, mints)))| {
                // End at the next boundary so cells stay contiguous across a DST
                // step — the FE click→filter tests this exact `[start, end)`.
                let end = ordered.get(i + 1).copied().unwrap_or(key + step);
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
                        .timestamp_millis_opt(end)
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
    fn no_entry_rows_do_not_inflate_n_fired() {
        // Matched-but-never-entered ride in the Positions payload (parity with
        // the sweep drill-in) but must not count toward fire-rate / PnL.
        let rows = vec![
            sim_row(1.0, 50.0, "TakeProfit", Some(10)),
            json!({
                "mint_address": "m", "symbol": "S",
                "fired": false, "exit_reason": "NoEntry",
                "pnl_sol": null, "pnl_percent": null, "holding_secs": null,
            }),
        ];
        let m = summarize(&rows).realized;
        assert_eq!(m.n_fired, 1);
        assert_eq!(m.n_closed, 1);
        assert!((m.total_pnl_sol - 1.0).abs() < 1e-9);
    }

    #[test]
    fn count_matched_dedupes_reentry_rows_but_not_n_fired() {
        // A re-entry rule (`RuleParams.reentry`) can fire several episodes on the
        // same mint — `run_replay` emits one row per episode. `n_fired` (a
        // position count) rightly counts every entry; `count_matched` (a token
        // count) must not — same mint, several rows, one matched token.
        let rows = vec![
            sim_row(1.0, 50.0, "TakeProfit", Some(10)),
            sim_row(-1.0, -50.0, "StopLoss", Some(20)),
            json!({
                "mint_address": "other", "symbol": "S2",
                "fired": false, "exit_reason": "NoEntry",
                "pnl_sol": null, "pnl_percent": null, "holding_secs": null,
            }),
        ];
        assert_eq!(summarize(&rows).realized.n_fired, 2, "two episodes, both counted");
        assert_eq!(count_matched(&rows), 2, "two distinct mints, re-entry not double-counted");
        assert_eq!(
            count_tokens_entered(&rows),
            1,
            "one mint fired twice is still one token entered",
        );
    }

    #[test]
    fn count_tokens_entered_excludes_no_entry_rows() {
        // A matched-but-never-entered candidate must not count as a "token
        // entered" — that distinction is the whole point of the two counters.
        let rows = vec![
            sim_row(1.0, 50.0, "TakeProfit", Some(10)),
            json!({
                "mint_address": "never", "symbol": "S2",
                "fired": false, "exit_reason": "NoEntry",
                "pnl_sol": null, "pnl_percent": null, "holding_secs": null,
            }),
        ];
        assert_eq!(count_matched(&rows), 2, "both mints matched");
        assert_eq!(count_tokens_entered(&rows), 1, "only one mint actually entered");
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
                exit_metric_slot: None,
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
        let body = time_summary(&rows, WallTimeField::EntryTime, None, None, chrono_tz::UTC);
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

        let forced = time_summary(
            &rows,
            WallTimeField::EntryTime,
            Some("1h"),
            Some("dense_15s"),
            chrono_tz::UTC,
        );
        assert_eq!(forced["wallGrain"], "1h");
        assert_eq!(forced["wallGrainAuto"], "30m");
        assert_eq!(forced["holdScheme"], "dense_15s");
        assert_eq!(forced["holdSchemeAuto"], "dense_60s");
    }

    /// Wall buckets are CIVIL buckets. These vectors are duplicated verbatim in
    /// the FE twin (`temporalSummary.test.ts` → "floors wall buckets in the app
    /// timezone"); the two folds must agree cell-for-cell or the Wall clock card
    /// and the Timing calendar next to it silently disagree about which day a
    /// position belongs to.
    #[test]
    fn wall_buckets_floor_in_the_requested_zone() {
        let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
        let kolkata: chrono_tz::Tz = "Asia/Kolkata".parse().unwrap();
        let at = |s: &str| parse_rfc3339_ms(s).unwrap();

        // Late-evening UTC instant is still the PREVIOUS civil day in New York.
        assert_eq!(
            floor_to_grain_in_zone(at("2026-01-15T02:30:00Z"), WallGrain::Day, ny),
            at("2026-01-14T05:00:00Z")
        );
        // 4h grain aligns to LOCAL 00/04/08/…, not to the UTC epoch.
        assert_eq!(
            floor_to_grain_in_zone(at("2026-07-15T14:37:00Z"), WallGrain::H4, ny),
            at("2026-07-15T12:00:00Z")
        );
        // Half-hour zone: an epoch-aligned floor is wrong at EVERY grain here.
        assert_eq!(
            floor_to_grain_in_zone(at("2026-07-15T14:20:00Z"), WallGrain::H1, kolkata),
            at("2026-07-15T13:30:00Z")
        );
        // DST fall-back day: the instant is EST (-5) but local midnight that day
        // was still EDT (-4) — the second pass is what gets this right.
        assert_eq!(
            floor_to_grain_in_zone(at("2026-11-01T12:00:00Z"), WallGrain::Day, ny),
            at("2026-11-01T04:00:00Z")
        );
        // UTC degrades to plain epoch alignment (the pre-fix behavior).
        let t = at("2026-07-15T14:37:00Z");
        assert_eq!(
            floor_to_grain_in_zone(t, WallGrain::H4, chrono_tz::UTC),
            t - t.rem_euclid(WallGrain::H4.step_ms())
        );
    }

    /// `exit_time` is the FE default. An open position has no exit stamp but is
    /// still an outcome on the timeline — it must fall back to `entry_time`, not
    /// vanish from the wall total (twin of FE `wallTimeMs`).
    #[test]
    fn exit_time_binning_keeps_open_positions_at_their_buy() {
        let rows = vec![
            json!({
                "mint_address":"a","exit_reason":"TakeProfit","holding_secs":30,"pnl_sol":1.0,
                "entry_time":"2026-07-15T14:00:00Z","exit_time":"2026-07-15T14:30:00Z"
            }),
            json!({
                "mint_address":"b","exit_reason":"Open","holding_secs":0,"pnl_sol":0.1,
                "entry_time":"2026-07-15T15:00:00Z","exit_time": Value::Null
            }),
        ];
        let body = time_summary(
            &rows,
            WallTimeField::ExitTime,
            Some("30m"),
            None,
            chrono_tz::UTC,
        );
        let cells = body["wall"].as_array().unwrap();
        let total: i64 = cells.iter().map(|c| c["n"].as_i64().unwrap_or(0)).sum();
        assert_eq!(total, 2, "the open position must still land on the timeline");
        // Closed row sits at its EXIT (14:30), not its entry (14:00).
        let at_1430 = cells
            .iter()
            .find(|c| c["start"].as_str().unwrap_or("").contains("14:30"))
            .expect("14:30 bucket");
        assert_eq!(at_1430["n"], 1);
    }

    /// A DST day is 23h or 25h, so a `t += step` cell grid drifts off the real
    /// boundaries and drops every row landing past the transition.
    #[test]
    fn no_row_is_dropped_across_a_dst_transition() {
        let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
        let stamps = [
            "2026-10-30T18:00:00Z",
            "2026-10-31T18:00:00Z",
            "2026-11-01T02:00:00Z", // before the 06:00Z fall-back
            "2026-11-01T18:00:00Z", // after it
            "2026-11-02T18:00:00Z",
            "2026-11-03T18:00:00Z",
        ];
        let rows: Vec<Value> = stamps
            .iter()
            .map(|ts| {
                json!({
                    "mint_address": "m", "exit_reason": "TakeProfit",
                    "holding_secs": 30, "pnl_sol": 1.0, "entry_time": ts
                })
            })
            .collect();
        let body = time_summary(&rows, WallTimeField::EntryTime, Some("day"), None, ny);
        let cells = body["wall"].as_array().unwrap();
        let total: i64 = cells.iter().map(|c| c["n"].as_i64().unwrap_or(0)).sum();
        assert_eq!(total, stamps.len() as i64, "every row must land in a cell");
        // Cells are contiguous: each end is the next start.
        for pair in cells.windows(2) {
            assert_eq!(pair[0]["end"], pair[1]["start"]);
        }
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
    fn hide_not_fired_filter_matches_simulate_toggle() {
        // Exact wire shape SimulatePage injects for "Hide not fired":
        // `filters.exit_reason = { op: "neq", val: "NoEntry" }`.
        let rows = vec![
            json!({"mint_address":"a","fired":true,"exit_reason":"TakeProfit"}),
            json!({"mint_address":"b","fired":false,"exit_reason":"NoEntry"}),
            json!({"mint_address":"c","fired":true,"exit_reason":"Open"}),
        ];
        let r = req(json!({"filters": {"exit_reason": {"op":"neq","val":"NoEntry"}}}));
        let (page, total) = query(&rows, &r);
        assert_eq!(total, 2, "neq NoEntry keeps fired + still-open");
        let mints: Vec<&str> = page.iter().filter_map(|r| r["mint_address"].as_str()).collect();
        assert_eq!(mints, vec!["a", "c"]);
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
