//! Phase 3 — one-shot backtest validation for the flow-reversion "dip scalper"
//! (results + verdict in `docs/history/2026-09-03-refuted-lines-ledger.md`). Drives the REAL
//! engine fold (`run_replay`) over the sealed Parquet lake for two rules — a 2-metric
//! MINIMAL CORE and a GATED variant — and prints the acceptance-gate stats vs the
//! family distributions recorded in `docs/history/2026-09-03-refuted-lines-ledger.md`.
//!
//! Ignored by default (reads real lake data + runs the full fold — not a unit test).
//! Run explicitly, single-threaded, with output:
//!   cargo test -p hunter-lab --test flow_scalper_validation -- --ignored --nocapture
//!
//! Lake path + cost model are the same the live `engine_sim` uses, so these numbers
//! are what a saved-rule simulate would report. Costs ARE modelled (this corrects
//! the plan's "no fee knob" premise): `CostModel::pumpfun_with_impact()` charges
//! 1.25%/leg fee + our own `B/vsol` footprint + tip + priority fee, applied to
//! realized PnL at close by `round_trip_with_costs`.
//! CAUTION (2026-07-28): the fee constant used here is 100 bps/leg and this cost model
//! charges no price impact — both since corrected, see
//! `docs/plans/strategies/execution-costs.md`. Numbers from this harness predate that
//! fix and are optimistic by the same margin as everything else pre-2026-07-28.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, DateTime, Utc};
use uuid::Uuid;

use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria, Fingerprint, FingerprintId};
use hunter_engine::grouping::TokenFingerprint;
use hunter_engine::metrics::price_window::PriceWindowState;
use hunter_engine::rule_params::RuleParams;

use lab::sweep::corpus::{CorpusSource, CorpusToken, Selection, TradeWindow};
use lab::sweep::projection::to_trade_lite;
use lab::lake::duck::LakeSource;
use lab::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};

use trading_core::strategies::kernel::CostModel;
use trading_core::strategies::paper_fill::FillModel;

/// Absolute lake path (test CWD is the crate dir, so a relative `./lake-data` from
/// `.env` would not resolve). Override with `FLOW_SCALPER_LAKE`.
fn lake_root() -> String {
    std::env::var("FLOW_SCALPER_LAKE")
        .unwrap_or_else(|_| "C:/Users/User/Documents/Bot/hunter/lake-data".to_string())
}

const BUY_SOL: f64 = 1.0;
const FP_CU: u64 = 200_000;
const FP_ID: u128 = 0xF10A_5CA1;
const WINDOW_SEC: u64 = 30;

/// A broad fingerprint every token matches — see [`uniform_tf`]. `tf` feeds ONLY
/// fingerprint matching, never a metric, so pinning every token to one matchable
/// shape makes the whole corpus arm; the rule's metric gates do the real filtering
/// (the analysis doc's universe is defined by age/liquidity/flow, not creation shape).
fn broad_fp() -> Fingerprint {
    Fingerprint {
        id: FingerprintId(Uuid::from_u128(FP_ID)),
        wildcard: false,
        criteria: Criteria::new()
            .with(AxisId::CuLimit, AxisPredicate::exact(u128::from(FP_CU))),
        metric_config: serde_json::json!({}),
    }
}

fn uniform_tf() -> TokenFingerprint {
    TokenFingerprint { cu_limit: Some(FP_CU), ..Default::default() }
}

fn rule(id: u128, params: serde_json::Value) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::from_u128(id)),
        fingerprint_id: FingerprintId(Uuid::from_u128(FP_ID)),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: (BUY_SOL * 1_000_000_000.0) as u64,
        // Effectively uncapped: measure the raw per-token edge (the sweep's B5
        // semantics), so the caps don't hide episodes. Re-entry lifecycle is out of scope here.
        max_concurrent_tokens: 1_000_000,
        max_total_tokens: 0,
        params: RuleParams::parse(&params).expect("valid rule params"),
        entry_enabled: true,
    }
}

/// MINIMAL CORE — the irreducible 2-metric loop: buy a `dip`% dip vs the `win`s high,
/// trail out at `retrace`% off the since-entry peak. Nothing else (knife-catches).
fn minimal_core(dip: f64, win: u64, retrace: f64) -> serde_json::Value {
    serde_json::json!({
        "entry": { "m_price_window": { "window_size_sec": win, "trail": [{"operator": ">=", "value": dip}] } },
        "exit":  { "m_position": { "retrace": [{"operator": ">=", "value": retrace}] } }
    })
}

/// GATED — the core plus the universe filter (age/liquidity/hot-flow) and the safety
/// exits (catastrophe stop via stop_loss=25 → pnl<=-25, long-stall net).
fn gated(dip: f64, win: u64, retrace: f64) -> serde_json::Value {
    serde_json::json!({
        "stop_loss": 25,
        "entry": {
            "m_state": {
                "time": [{"operator": ">=", "value": 120}],
                "liquidity": [{"operator": ">=", "value": 45}, {"operator": "<=", "value": 110}]
            },
            "m_price_window": { "window_size_sec": win, "trail": [{"operator": ">=", "value": dip}] },
            "m_flow_window": { "window_size_sec": win, "gross_flow": [{"operator": ">=", "value": 10}] }
        },
        "exit": {
            "m_position": { "retrace": [{"operator": ">=", "value": retrace}] },
            "m_price_lifetime": { "stall": [{"operator": ">=", "value": 15}] }
        }
    })
}

/// GATED + the sell-EXHAUSTION refinement: age/liquidity + dip, and a short **2 s
/// net-flow floor** on entry — "the immediate dump is pausing" (analysis doc: net
/// flow 1-5 s before omego's entry is negative but flips positive 5 s after; buying
/// only once the short-window sell pressure has eased should skip falling knives).
///
/// This variant TRADES the 30 s `gross_flow` hot gate for the 2 s `net_flow` gate —
/// the "one-for-the-other" comparison the old single-`m_flow_window`-per-side schema
/// forced. Age + liquidity still bound the universe. Contrast [`gated_combined`],
/// which keeps BOTH now that the schema allows multiple windows per group per side.
fn gated_exhaustion(dip: f64, win: u64, retrace: f64, netflow_floor: f64) -> serde_json::Value {
    serde_json::json!({
        "stop_loss": 25,
        "entry": {
            "m_state": {
                "time": [{"operator": ">=", "value": 120}],
                "liquidity": [{"operator": ">=", "value": 45}, {"operator": "<=", "value": 110}]
            },
            "m_price_window": { "window_size_sec": win, "trail": [{"operator": ">=", "value": dip}] },
            "m_flow_window": { "window_size_sec": 2, "net_flow": [{"operator": ">=", "value": netflow_floor}] }
        },
        "exit": {
            "m_position": { "retrace": [{"operator": ">=", "value": retrace}] },
            "m_price_lifetime": { "stall": [{"operator": ">=", "value": 15}] }
        }
    })
}

/// GATED + the FULL exhaustion refinement — lever #1: keep BOTH the `win`-second
/// `gross_flow` hot gate (there's live flow) AND the 2 s `net_flow` floor (the dump
/// has paused) on entry, now that a side can hold **multiple `m_flow_window` windows**
/// (the array shape this branch adds). This is the refinement the old probe could only
/// approximate by dropping one gate; here it runs as authored.
fn gated_combined(dip: f64, win: u64, retrace: f64, netflow_floor: f64) -> serde_json::Value {
    serde_json::json!({
        "stop_loss": 25,
        "entry": {
            "m_state": {
                "time": [{"operator": ">=", "value": 120}],
                "liquidity": [{"operator": ">=", "value": 45}, {"operator": "<=", "value": 110}]
            },
            "m_price_window": { "window_size_sec": win, "trail": [{"operator": ">=", "value": dip}] },
            "m_flow_window": [
                { "window_size_sec": win, "gross_flow": [{"operator": ">=", "value": 10}] },
                { "window_size_sec": 2,   "net_flow":   [{"operator": ">=", "value": netflow_floor}] }
            ]
        },
        "exit": {
            "m_position": { "retrace": [{"operator": ">=", "value": retrace}] },
            "m_price_lifetime": { "stall": [{"operator": ">=", "value": 15}] }
        }
    })
}

/// GATED with an OPTIONAL long-stall safety net (right-tail probe). The one-shot
/// diagnosis was that our holds cap at 4-8s vs omego's 17s, clipping the thin right
/// tail that IS the edge. The 15s `stall` net is a prime suspect for firing before a
/// winner's reversion develops. `stall_sec = None` drops it entirely (retrace + the
/// -25 catastrophe stop remain the only exits); `Some(s)` sets the net at `s` seconds.
fn gated_stall(dip: f64, win: u64, retrace: f64, stall_sec: Option<f64>) -> serde_json::Value {
    let mut exit = serde_json::json!({
        "m_position": { "retrace": [{"operator": ">=", "value": retrace}] }
    });
    if let Some(s) = stall_sec {
        exit["m_price_lifetime"] = serde_json::json!({ "stall": [{"operator": ">=", "value": s}] });
    }
    serde_json::json!({
        "stop_loss": 25,
        "entry": {
            "m_state": {
                "time": [{"operator": ">=", "value": 120}],
                "liquidity": [{"operator": ">=", "value": 45}, {"operator": "<=", "value": 110}]
            },
            "m_price_window": { "window_size_sec": win, "trail": [{"operator": ">=", "value": dip}] },
            "m_flow_window": { "window_size_sec": win, "gross_flow": [{"operator": ">=", "value": 10}] }
        },
        "exit": exit
    })
}

fn load_corpus() -> Vec<CorpusToken> {
    let sel = Selection {
        mints: None,
        token_cap: 1_000_000, // all tokens across every sealed day
        created_after: None,
        created_before: None,
        per_mint_cap: i64::MAX, // full history per token
        window: TradeWindow::LaunchWindow,
        curve_only: true, // bonding-curve only — the strategy's universe
        with_signatures: false,
        with_flow: false,
        with_flow_text: false,
        with_oracle: false,
    };
    let src = LakeSource::new(lake_root());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let corpus = rt.block_on(src.load(&sel)).expect("lake load");
    eprintln!(
        "loaded corpus: {} tokens, {} trades",
        corpus.token_count(),
        corpus.trade_count()
    );
    corpus.tokens
}

fn to_replay_tokens(tokens: &[CorpusToken]) -> Vec<ReplayToken> {
    tokens
        .iter()
        .map(|t| ReplayToken {
            mint: t.mint.clone(),
            symbol: t.symbol.clone(),
            created_at: t.created_at,
            tf: uniform_tf(),
            trades: Arc::clone(&t.trades),
            creator_wallet_hash: None,
            identity: t.identity,
            creation_slot: None,
        })
        .collect()
}

/// The rolling-window `trail` (dip depth vs the 30s high) at the entry trigger —
/// re-derived by folding the token's trades (same `to_trade_lite` price the engine
/// uses) into a `PriceWindowState(30)` up to the trigger time. Informational: the
/// entry condition already forces `trail >= 12`, so this shows the realized shape.
fn entry_dip_depth(tokens: &HashMap<String, Arc<Vec<lab::sweep::projection::CorpusTrade>>>, o: &PositionOutcome) -> Option<f64> {
    let trades = tokens.get(&o.mint)?;
    let target = o.target_time?;
    let mut pw = PriceWindowState::new(hunter_engine::metrics::WindowSpec::secs(WINDOW_SEC as f64));
    let mut last = None;
    for ct in trades.iter() {
        if ct.block_time > target {
            break;
        }
        let t = to_trade_lite(ct);
        let at_ms = t.at.timestamp_millis();
        pw.on_trade(t.price, at_ms, at_ms);
        last = Some(pw.trail(at_ms));
    }
    last.filter(|v| v.is_finite())
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn median(sorted: &[f64]) -> f64 {
    pct(sorted, 0.5)
}

/// Is this entry in the omego busy window (the wallet-attributed 07-20/07-21 slice)?
fn busy_window(entry_time: DateTime<Utc>) -> bool {
    let d = entry_time.date_naive();
    d.year() == 2026 && d.month() == 7 && (d.day() == 20 || d.day() == 21)
}

struct Report {
    label: &'static str,
    fired: usize,
    closed: usize,
    open: usize,
    win_rate_before: f64,
    win_rate_after: f64,
    hold_med: f64,
    hold_p25: f64,
    hold_p75: f64,
    dip_med: f64,
    pnl_med_after: f64,
    p10_after: f64,
    total_realized_after: f64,
    total_mtm_after: f64,
    total_realized_before: f64,
    // Fee-only accounting (fee + tip + priority, NO cost-model slippage) — the
    // correct pairing with an explicit fill model, which already prices execution
    // slippage. `total_realized_after` (full model) double-counts slippage when a
    // fill model is in play; this is the honest bottom line.
    win_rate_feeonly: f64,
    pnl_med_feeonly: f64,
    total_realized_feeonly: f64,
    busy_realized_after: f64,
    busy_closed: usize,
    exit_mix: Vec<(String, usize)>,
}

/// Default (worst-case) fill — what live paper books; the existing probes use this.
fn run(label: &'static str, params: serde_json::Value, tokens: &[CorpusToken]) -> Report {
    run_with_fill(label, params, tokens, FillModel::WorstCase)
}

/// Run one rule under a specific [`FillModel`] (lever #2: fill-sensitivity). The
/// taken-position SET is identical across models — only the fill PRICE differs — so
/// the delta isolates how much of the modeled loss is fill pessimism vs no edge.
fn run_with_fill(
    label: &'static str,
    params: serde_json::Value,
    tokens: &[CorpusToken],
    fill: FillModel,
) -> Report {
    let fp = broad_fp();
    let r = rule(1, params);
    let replay_tokens = to_replay_tokens(tokens);
    let as_of = Utc::now();
    let outcomes = run_replay(&[r], &[fp], replay_tokens, ReplayConfig { as_of, fill_model: fill, ..Default::default() });

    let trade_map: HashMap<String, Arc<Vec<lab::sweep::projection::CorpusTrade>>> =
        tokens.iter().map(|t| (t.mint.clone(), Arc::clone(&t.trades))).collect();

    // `realImp` — fee + tip + our own `B/vsol` footprint, sized off each outcome's
    // own `entry_reserve_sol`. This column used to print a flat 1%/leg stand-in; that
    // model is retired, so numbers this harness published before the switch do not
    // compare to this one.
    let costed = CostModel::pumpfun_with_impact();
    // `realFee` — the honest column, and the one the sweep's `cost_model` selector
    // must reproduce. `sweep_cost_selector_matches_the_realfee_column` locks the two
    // to the same `CostModel` so a sweep run under `pumpfun_fee_only` + a fill model
    // can be read against this harness's numbers directly.
    let feeonly = CostModel::pumpfun_fee_only();
    let free = CostModel::frictionless();

    let mut closed_pnl_after: Vec<f64> = Vec::new();
    let mut closed_pnl_pct_after: Vec<f64> = Vec::new();
    let mut closed_pnl_pct_feeonly: Vec<f64> = Vec::new();
    let mut closed_hold: Vec<f64> = Vec::new();
    let mut dips: Vec<f64> = Vec::new();
    let mut wins_before = 0usize;
    let mut wins_after = 0usize;
    let mut wins_feeonly = 0usize;
    let mut open = 0usize;
    let mut total_realized_after = 0.0;
    let mut total_realized_before = 0.0;
    let mut total_realized_feeonly = 0.0;
    let mut total_mtm_after = 0.0;
    let mut busy_realized_after = 0.0;
    let mut busy_closed = 0usize;
    let mut exit_mix: HashMap<String, usize> = HashMap::new();

    for o in &outcomes {
        if let Some(d) = entry_dip_depth(&trade_map, o) {
            dips.push(d);
        }
        let exited = o.exit_reason.is_some();
        let (pnl_after, pct_after) = o.pnl_with_costs(BUY_SOL, &costed);
        let (pnl_feeonly, pct_feeonly) = o.pnl_with_costs(BUY_SOL, &feeonly);
        let (pnl_before, _) = o.pnl_with_costs(BUY_SOL, &free);
        if exited {
            let reason = o.exit_reason.map(|r| r.label().into_owned()).unwrap_or_default();
            *exit_mix.entry(reason).or_default() += 1;
            closed_pnl_after.push(pnl_after);
            closed_pnl_pct_after.push(pct_after);
            closed_pnl_pct_feeonly.push(pct_feeonly);
            let hold = o.exit_time.map(|t| (t - o.entry_time).num_milliseconds() as f64 / 1000.0).unwrap_or(0.0);
            closed_hold.push(hold);
            total_realized_after += pnl_after;
            total_realized_feeonly += pnl_feeonly;
            total_realized_before += pnl_before;
            if pnl_before > 0.0 {
                wins_before += 1;
            }
            if pnl_after > 0.0 {
                wins_after += 1;
            }
            if pnl_feeonly > 0.0 {
                wins_feeonly += 1;
            }
            if busy_window(o.entry_time) {
                busy_realized_after += pnl_after;
                busy_closed += 1;
            }
        } else {
            open += 1;
        }
        total_mtm_after += pnl_after;
    }

    closed_hold.sort_by(|a, b| a.partial_cmp(b).unwrap());
    closed_pnl_pct_after.sort_by(|a, b| a.partial_cmp(b).unwrap());
    closed_pnl_pct_feeonly.sort_by(|a, b| a.partial_cmp(b).unwrap());
    dips.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let closed = closed_pnl_after.len();
    let mut exit_mix: Vec<(String, usize)> = exit_mix.into_iter().collect();
    exit_mix.sort_by(|a, b| b.1.cmp(&a.1));

    Report {
        label,
        fired: outcomes.len(),
        closed,
        open,
        win_rate_before: if closed == 0 { 0.0 } else { wins_before as f64 / closed as f64 },
        win_rate_after: if closed == 0 { 0.0 } else { wins_after as f64 / closed as f64 },
        hold_med: median(&closed_hold),
        hold_p25: pct(&closed_hold, 0.25),
        hold_p75: pct(&closed_hold, 0.75),
        dip_med: median(&dips),
        pnl_med_after: median(&closed_pnl_pct_after),
        p10_after: pct(&closed_pnl_pct_after, 0.10),
        total_realized_after,
        total_mtm_after,
        total_realized_before,
        win_rate_feeonly: if closed == 0 { 0.0 } else { wins_feeonly as f64 / closed as f64 },
        pnl_med_feeonly: median(&closed_pnl_pct_feeonly),
        total_realized_feeonly,
        busy_realized_after,
        busy_closed,
        exit_mix,
    }
}

fn print_report(r: &Report) {
    eprintln!("\n================ {} ================", r.label);
    eprintln!("fired={}  closed={}  open={}", r.fired, r.closed, r.open);
    eprintln!(
        "win rate  before-costs={:.1}%   after-costs={:.1}%   (gate: 55-70% before)",
        r.win_rate_before * 100.0,
        r.win_rate_after * 100.0
    );
    eprintln!(
        "hold secs  p25={:.1}  med={:.1}  p75={:.1}   (gate: median in [5,60])",
        r.hold_p25, r.hold_med, r.hold_p75
    );
    eprintln!(
        "entry dip depth vs 30s high (taken)  median={:.1}%   (family band -8..-20%; >=12 by construction)",
        r.dip_med
    );
    eprintln!(
        "episode pnl%  median={:.2}%   p10={:.2}%   (gate: p10 >= -25%)",
        r.pnl_med_after, r.p10_after
    );
    eprintln!(
        "TOTAL realized PnL (after ~4%/round costs)  = {:+.3} SOL   (before costs {:+.3})",
        r.total_realized_after, r.total_realized_before
    );
    eprintln!(
        "TOTAL realized PnL (FEE-ONLY: fee+tip+priority, no double-counted slippage) = {:+.3} SOL  \
         win={:.1}%  pnlMed={:.2}%  <- honest bottom line for an explicit fill model",
        r.total_realized_feeonly, r.win_rate_feeonly * 100.0, r.pnl_med_feeonly
    );
    eprintln!("TOTAL mark-to-market PnL (incl. open marks) = {:+.3} SOL", r.total_mtm_after);
    eprintln!(
        "BUSY-WINDOW (07-20/07-21) realized PnL after costs = {:+.3} SOL over {} closed  (gate: positive)",
        r.busy_realized_after, r.busy_closed
    );
    eprintln!("exit mix: {:?}", r.exit_mix);
}

fn summary_line(r: &Report) {
    eprintln!(
        "{:<34} fired={:>5} closed={:>5} winB={:>5.1}% winFee={:>5.1}% holdMed={:>5.1}s dipMed={:>5.1}% pnlMed={:>6.2}% p10={:>7.2}% realFee={:>+8.2} realImp={:>+8.2} realFree={:>+8.2} SOL",
        r.label, r.fired, r.closed, r.win_rate_before * 100.0, r.win_rate_feeonly * 100.0,
        r.hold_med, r.dip_med, r.pnl_med_after, r.p10_after,
        r.total_realized_feeonly, r.total_realized_after, r.total_realized_before,
    );
}

/// The sweep's wire cost-model selector must resolve to the **same** `CostModel`
/// this harness prints as `realFee` — otherwise a grid run under `pumpfun_fee_only`
/// could not be compared against the numbers these probes report, which is the whole
/// point of exposing the selector. No lake, no fold: a pure wiring lock, so it runs
/// on every `cargo test` rather than only under `--ignored`.
#[test]
fn sweep_cost_selector_matches_the_realfee_column() {
    use trading_core::strategies::kernel::CostModelKind;

    let selected = CostModelKind::PumpfunFeeOnly.model();
    let harness = CostModel::pumpfun_fee_only();
    assert_eq!(selected.fee_bps_per_leg, harness.fee_bps_per_leg);
    assert_eq!(selected.fixed_cost_sol_per_leg, harness.fixed_cost_sol_per_leg);
    assert!(!selected.price_impact, "realFee is the zero-impact bound");
    // An omitted field takes the size-aware model, so a caller cannot land on a
    // zero-impact number by forgetting to name one.
    assert_eq!(CostModelKind::default(), CostModelKind::PumpfunImpact);
    assert!(CostModelKind::default().model().price_impact);
}

#[test]
#[ignore = "reads the real sealed lake + runs the full replay fold; run with --ignored --nocapture"]
fn flow_scalper_one_shot_validation() {
    let tokens = load_corpus();
    assert!(!tokens.is_empty(), "empty corpus — check the lake path");

    // The knife-catch reference (no universe filter), then the gated grid. The grid
    // isolates the diagnosed culprit — the trailing-stop width vs feed-reactive fills
    // (the sim's worst-case fill noise-stops a too-tight trail before reversion) —
    // plus a dip-depth and a window probe, over the blueprint ranges (analysis doc).
    let min_ref = run("MIN core d12/w30/r3", minimal_core(12.0, 30, 3.0), &tokens);
    let grid: &[(&'static str, f64, u64, f64)] = &[
        ("GATED d12/w30/r3",  12.0, 30, 3.0),
        ("GATED d12/w30/r5",  12.0, 30, 5.0),
        ("GATED d12/w30/r8",  12.0, 30, 8.0),
        ("GATED d12/w30/r12", 12.0, 30, 12.0),
        ("GATED d15/w30/r5",  15.0, 30, 5.0),
        ("GATED d12/w60/r5",  12.0, 60, 5.0),
    ];
    let reports: Vec<Report> = grid
        .iter()
        .map(|&(label, dip, win, retrace)| run(label, gated(dip, win, retrace), &tokens))
        .collect();

    print_report(&min_ref);
    for r in &reports {
        print_report(r);
    }
    eprintln!("\n==================== ONE-LINE SUMMARY (net of ~4%/round costs) ====================");
    summary_line(&min_ref);
    for r in &reports {
        summary_line(r);
    }

    assert!(min_ref.fired > 0, "no entries fired — check metrics wiring");
}

/// Separate, smaller probe of the top recommended lever — the sell-exhaustion entry
/// gate — vs a comparison baseline, so it can run without the full 7-config grid.
///   cargo test -p hunter-lab --test flow_scalper_validation exhaustion -- --ignored --nocapture
#[test]
#[ignore = "reads the real sealed lake + runs the full replay fold; run with --ignored --nocapture"]
fn flow_scalper_exhaustion_probe() {
    let tokens = load_corpus();
    assert!(!tokens.is_empty(), "empty corpus — check the lake path");

    // Comparison baseline (best grid config: deeper dip + 5% trail, 30s hot gate).
    let base = run("GATED d15/w30/r5 (30s gross gate)", gated(15.0, 30, 5.0), &tokens);
    // Exhaustion, one-for-the-other: trade the 30s gross gate for a 2s net-flow floor.
    let exh_m1 = run("EXH d15/r5 netflow(2s)>=-1", gated_exhaustion(15.0, 30, 5.0, -1.0), &tokens);
    let exh_0 = run("EXH d15/r5 netflow(2s)>=0", gated_exhaustion(15.0, 30, 5.0, 0.0), &tokens);
    // COMBINED (lever #1): keep BOTH the 30s gross hot gate and the 2s net-flow floor
    // — the full refinement the multi-window schema now allows in one rule.
    let cmb_m1 = run("CMB d15/r5 gross(30s)+netflow(2s)>=-1", gated_combined(15.0, 30, 5.0, -1.0), &tokens);
    let cmb_0 = run("CMB d15/r5 gross(30s)+netflow(2s)>=0", gated_combined(15.0, 30, 5.0, 0.0), &tokens);

    print_report(&base);
    print_report(&exh_m1);
    print_report(&exh_0);
    print_report(&cmb_m1);
    print_report(&cmb_0);
    eprintln!("\n==================== EXHAUSTION PROBE SUMMARY (net of ~4%/round costs) ====================");
    summary_line(&base);
    summary_line(&exh_m1);
    summary_line(&exh_0);
    summary_line(&cmb_m1);
    summary_line(&cmb_0);

    // Sanity: the combined rule actually fires (both windows registered + evaluated).
    assert!(cmb_0.fired > 0, "combined-gate rule fired nothing — multi-window wiring bug");
}

/// Deeper-dip sweep — the one untested lever the family distribution points at. The
/// one-shot grid only ran dip 12/15; win% and realized-after both improved 12->15, and
/// the profitable-dip-buyer family concentrates at p25 -28% / med -14.3% vs the 30s
/// high (analysis doc). Two hypotheses in one run:
///   (A) DEEPER DIP: does selecting for deeper drawdowns (fewer, higher-quality
///       knife-catches nearer the local bottom) push per-episode edge above breakeven?
///       Sweep dip {15,20,25,28} at the best trail (r5) and a wider one (r8, since a
///       deeper dip may bounce further before the trail should engage).
///   (B) RIGHT-TAIL CAPTURE: our holds cap ~4-8s vs omego's 17s. Re-run the best deep
///       dip with the 15s stall net LOOSENED (30s) and DROPPED (none) to see if the
///       median hold and p90 pnl move toward omego's — i.e. is the stall net clipping
///       winners before reversion? (Wider retrace already failed this; the exit MIX,
///       not the trail width, is the suspect.)
///   cargo test -p hunter-lab --test flow_scalper_validation deep_dip -- --ignored --nocapture
#[test]
#[ignore = "reads the real sealed lake + runs the full replay fold; run with --ignored --nocapture"]
fn flow_scalper_deep_dip() {
    let tokens = load_corpus();
    assert!(!tokens.is_empty(), "empty corpus — check the lake path");

    // (A) deeper-dip sweep at r5 (best trail) and r8 (room for a deeper bounce).
    let dip_grid: &[(&'static str, f64, u64, f64)] = &[
        ("GATED d15/w30/r5", 15.0, 30, 5.0),
        ("GATED d20/w30/r5", 20.0, 30, 5.0),
        ("GATED d25/w30/r5", 25.0, 30, 5.0),
        ("GATED d28/w30/r5", 28.0, 30, 5.0),
        ("GATED d20/w30/r8", 20.0, 30, 8.0),
        ("GATED d25/w30/r8", 25.0, 30, 8.0),
    ];
    let dip_reports: Vec<Report> = dip_grid
        .iter()
        .map(|&(label, dip, win, retrace)| run(label, gated(dip, win, retrace), &tokens))
        .collect();

    // (B) right-tail: best deep dip (d25/r8) with the 15s stall net at 15s / 30s / none.
    let tail_reports: Vec<Report> = vec![
        run("TAIL d25/r8 stall=15s", gated_stall(25.0, 30, 8.0, Some(15.0)), &tokens),
        run("TAIL d25/r8 stall=30s", gated_stall(25.0, 30, 8.0, Some(30.0)), &tokens),
        run("TAIL d25/r8 stall=none", gated_stall(25.0, 30, 8.0, None), &tokens),
    ];

    for r in &dip_reports {
        print_report(r);
    }
    for r in &tail_reports {
        print_report(r);
    }
    eprintln!("\n==================== DEEP-DIP + RIGHT-TAIL SUMMARY (net of ~4%/round costs) ====================");
    for r in dip_reports.iter().chain(tail_reports.iter()) {
        summary_line(r);
    }

    assert!(dip_reports[0].fired > 0, "no entries fired — check metrics wiring");
    // Dropping the stall net must not shrink the taken set (entry is unchanged) — it can
    // only change exits; guards against an accidental entry-side edit.
    assert!(
        tail_reports[2].closed + tail_reports[2].open >= tail_reports[0].closed,
        "dropping the stall net changed the taken set — entry side must be identical"
    );
}

/// Lever #2 — fill-realism sensitivity. The sim fills WORST-CASE adverse in the
/// post-signal slot window (entry = highest window buy, exit = lowest window price),
/// while omego lands same-slot. This probe reprices the SAME taken set under three
/// fill models to bound how much of the ~4%/round loss is fill pessimism vs a genuine
/// absence of edge:
///   * `worst`  — the current adverse bound (what live paper books);
///   * `first`  — the first print after the signal (a neutral "next trade" reaction);
///   * `signal` — the trigger/fire spot, zero slippage (the optimistic same-slot bound).
/// Read: if even `signal` is ≤0 the strategy is dead; if `worst`≤0 but `signal`>0 the
/// loss is fill/latency and the fix is execution (fast landing), not the entry logic.
///   cargo test -p hunter-lab --test flow_scalper_validation fill_sensitivity -- --ignored --nocapture
#[test]
#[ignore = "reads the real sealed lake + runs the full replay fold; run with --ignored --nocapture"]
fn flow_scalper_fill_sensitivity() {
    let tokens = load_corpus();
    assert!(!tokens.is_empty(), "empty corpus — check the lake path");

    // The two best configs (pure-gated best + the exhaustion best), each × 3 fill models.
    let runs: Vec<(&'static str, serde_json::Value, FillModel)> = vec![
        ("GATED d15/w30/r5 [worst] ", gated(15.0, 30, 5.0), FillModel::WorstCase),
        ("GATED d15/w30/r5 [first] ", gated(15.0, 30, 5.0), FillModel::FirstInWindow),
        ("GATED d15/w30/r5 [signal]", gated(15.0, 30, 5.0), FillModel::SignalPrice),
        ("EXH d15/r5 nf>=0 [worst] ", gated_exhaustion(15.0, 30, 5.0, 0.0), FillModel::WorstCase),
        ("EXH d15/r5 nf>=0 [first] ", gated_exhaustion(15.0, 30, 5.0, 0.0), FillModel::FirstInWindow),
        ("EXH d15/r5 nf>=0 [signal]", gated_exhaustion(15.0, 30, 5.0, 0.0), FillModel::SignalPrice),
    ];
    let reports: Vec<Report> = runs
        .into_iter()
        .map(|(label, params, model)| run_with_fill(label, params, &tokens, model))
        .collect();

    for r in &reports {
        print_report(r);
    }
    eprintln!("\n==================== FILL-SENSITIVITY SUMMARY (net of ~4%/round costs) ====================");
    for r in &reports {
        summary_line(r);
    }

    // The taken set must be identical across fill models within a config (only price
    // changes) — the whole premise of the controlled reprice.
    assert_eq!(reports[0].closed, reports[1].closed, "fill model changed the GATED taken set");
    assert_eq!(reports[1].closed, reports[2].closed, "fill model changed the GATED taken set");
    assert_eq!(reports[3].closed, reports[4].closed, "fill model changed the EXH taken set");
    assert_eq!(reports[4].closed, reports[5].closed, "fill model changed the EXH taken set");
}
