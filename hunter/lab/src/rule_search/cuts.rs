//! Cut table from this range's metric paths (`docs/arch/sweep.md` "Rule search").
//!
//! Labels are path facts (ran vs never-ran, dumpers), not a rule. Entry uses
//! peak contrast + winner floor when the split is thick. Run-lead (seconds
//! before first 1.5×), launch (first print), and fill-moment are extras — they
//! do not replace peak. Mixed early/peak only when the split is thin. Exit uses
//! dump-lead, giveback-lead (seconds after ATH), runner after-dump, dumper p90,
//! and held from fill-to-dump. Position bounce/pnl stay on a declared menu.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::flow_ix::FlowPatterns;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_of, is_fingerprint_scoped, metric_spec, MetricGroupId, MetricId, MetricKind, MetricScope,
    Unit, REGISTRY,
};
use trading_core::strategies::kernel::exact_quantile_f64;

use crate::discovery::candidates::round_for_unit;
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::axes::SWEEP_FLOW_FP;
use crate::sweep::projection::to_trade_lite;

use super::roles::{entry_role, is_position, EntryRole};

/// Seconds before ATH / before 1.5× / after ATH that count as a lead window.
const LEAD_SEC: i64 = 3;
/// Seconds after create that count as the launch snapshot window (first print).
const LAUNCH_SEC: i64 = 2;
/// Minimum tokens on each side before contrast / dump-lead prefer the split.
pub(crate) const MIN_SPLIT: usize = 8;
const DUMP_FRAC: f64 = 0.85;
const MIN_ATH_MULT: f64 = 1.5;
/// Admission gate: a clause must be false on at least this share of the
/// cohort's sampled rows, or it selects nothing (`untagged_buy >= 0`) and is
/// dropped before any combo is scored.
const MIN_REJECT_SHARE: f64 = 0.10;

/// Path phase a threshold was measured on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutPhase {
    Early,
    Peak,
    After,
    AfterDump,
    P90,
    Declared,
    Contrast,
    DumpLead,
    WinnerFloor,
    /// p90-of-ran ceiling — pairs with [`WinnerFloor`](Self::WinnerFloor) into a
    /// band (one can-fail filling), never a standalone single.
    WinnerCeil,
    DumperP90,
    FillMoment,
    Outcome,
    RunLead,
    Launch,
    GivebackLead,
}

impl CutPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Early => "early",
            Self::Peak => "peak",
            Self::After => "after",
            Self::AfterDump => "after-dump",
            Self::P90 => "p90",
            Self::Declared => "declared",
            Self::Contrast => "contrast",
            Self::DumpLead => "dump-lead",
            Self::WinnerFloor => "winner-floor",
            Self::WinnerCeil => "winner-ceil",
            Self::DumperP90 => "dumper-p90",
            Self::FillMoment => "fill-moment",
            Self::Outcome => "outcome",
            Self::RunLead => "run-lead",
            Self::Launch => "launch",
            Self::GivebackLead => "giveback-lead",
        }
    }

    /// Extra entry clocks that may sit beside peak contrast for the same metric.
    pub fn is_entry_extra(self) -> bool {
        matches!(self, Self::RunLead | Self::Launch | Self::FillMoment)
    }

    /// Lower is kept first when the generator truncates the menu.
    /// Peak contrast outranks extras so a 1.5× snapshot cannot erase
    /// the clock that times entries away from the rip. Run-lead outranks
    /// launch and fill-moment among extras.
    pub fn menu_rank(self) -> u8 {
        match self {
            Self::Contrast => 0,
            Self::WinnerFloor | Self::WinnerCeil => 1,
            Self::RunLead => 2,
            Self::DumpLead => 3,
            Self::GivebackLead => 4,
            Self::Launch => 5,
            Self::Outcome | Self::DumperP90 | Self::AfterDump => 6,
            Self::Peak => 7,
            Self::Early => 8,
            Self::FillMoment => 9,
            Self::After | Self::P90 => 10,
            Self::Declared => 11,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub group: MetricGroupId,
    pub metric: MetricId,
    pub window: Option<hunter_engine::metrics::WindowSpec>,
    pub op: Operator,
    pub threshold: f64,
    pub phase: CutPhase,
}

#[derive(Clone, Debug)]
pub struct CutTable {
    pub windows: Vec<f64>,
    pub entry: Vec<Cut>,
    pub exit: Vec<Cut>,
    /// One snapshot per ran token at fill-moment, keyed by `fill_key`.
    pub winner_fill: Vec<HashMap<(MetricId, i64), f64>>,
    /// Last row in the run-lead window (just before 1.5×) per ran token.
    pub winner_lead: Vec<HashMap<(MetricId, i64), f64>>,
    /// First-print snapshot per ran token.
    pub winner_launch: Vec<HashMap<(MetricId, i64), f64>>,
}

impl CutTable {
    pub fn empty() -> Self {
        Self {
            windows: Vec::new(),
            entry: Vec::new(),
            exit: Vec::new(),
            winner_fill: Vec::new(),
            winner_lead: Vec::new(),
            winner_launch: Vec::new(),
        }
    }
}

pub fn fill_key(metric: MetricId, window: Option<hunter_engine::metrics::WindowSpec>) -> (MetricId, i64) {
    (
        metric,
        window
            .map(|w| hunter_engine::metrics::quantize(w.size) as i64)
            .unwrap_or(-1),
    )
}

/// Declared position-scoped exit menus (no token-independent distribution).
const POSITION_EXIT: &[(MetricId, Operator, &[f64])] = &[
    (MetricId::Retrace, Operator::Gte, &[10.0, 15.0, 25.0]),
    (MetricId::Bounce, Operator::Gte, &[10.0, 15.0, 25.0]),
    (MetricId::Held, Operator::Gte, &[60.0, 120.0, 300.0]),
    (MetricId::Pnl, Operator::Gte, &[20.0, 40.0, 80.0]),
];

struct TokenPath {
    ath_price: f64,
    ath_at: DateTime<Utc>,
    ttp: f64,
    winner: bool,
    dumper: bool,
    first_price: f64,
    /// First time price reaches `MIN_ATH_MULT` × first, if ever.
    t15: Option<DateTime<Utc>>,
    dump_at: Option<DateTime<Utc>>,
}

/// Build the cut table for this cohort. `flow` / `flow_fp` enable split metrics.
pub fn build_cut_table(
    tokens: &[CorpusToken],
    flow: Option<&FlowPatterns>,
    flow_fp: FingerprintId,
) -> CutTable {
    if tokens.is_empty() {
        return CutTable::empty();
    }
    let paths = label_paths(tokens);
    let windows = cohort_windows(tokens, &paths);
    let columns = cut_columns(&windows, flow.is_some(), flow_fp);
    let sampled = sample_phases(tokens, &paths, &columns, flow, flow_fp);
    let thick = paths.iter().filter(|p| p.winner).count() >= MIN_SPLIT
        && paths.iter().filter(|p| !p.winner).count() >= MIN_SPLIT;
    let fill_n = samples_n(&sampled.by, Bucket::FillWinner);
    let run_lead_n = samples_n(&sampled.by, Bucket::RunLeadWin);
    let launch_n = samples_n(&sampled.by, Bucket::LaunchWin);
    let lead_n = samples_n(&sampled.by, Bucket::DumpLead);
    let dump_run_n = samples_n(&sampled.by, Bucket::AfterDumpRun);
    let gb_n = samples_n(&sampled.by, Bucket::GivebackLead);

    let mut entry = Vec::new();
    let mut exit = Vec::new();
    for (ci, col) in columns.iter().enumerate() {
        let (group, metric, window) = col_meta(*col);
        if is_position(metric) {
            continue;
        }
        let unit = metric_spec(metric).unit;
        if let Some(role) = entry_role(metric) {
            if !matches!(role, EntryRole::WaitOnly) {
                push_entry(
                    &mut entry,
                    &sampled.by,
                    ci,
                    group,
                    metric,
                    window,
                    unit,
                    role,
                    thick,
                    fill_n,
                    run_lead_n,
                    launch_n,
                );
            }
        }
        push_exit(
            &mut exit,
            &sampled.by,
            ci,
            group,
            metric,
            window,
            unit,
            lead_n,
            dump_run_n,
            gb_n,
        );
    }
    for &(id, op, vals) in POSITION_EXIT {
        if id == MetricId::Held && sampled.held_ran.len() >= MIN_SPLIT {
            continue;
        }
        let group = group_of(id).id;
        for &v in vals {
            exit.push(Cut {
                group,
                metric: id,
                window: None,
                op,
                threshold: v,
                phase: CutPhase::Declared,
            });
        }
    }
    push_outcome_held(&mut exit, &sampled.held_ran);

    // Vacuous-cut admission gate: drop clauses that reject (almost) nothing on
    // this cohort. Position-scoped / declared cuts have no sampled column and
    // are kept — the sample basis, not an exemption list, decides.
    let col_ix: HashMap<(MetricId, i64), usize> = columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            let (_, metric, window) = col_meta(*col);
            (fill_key(metric, window), ci)
        })
        .collect();
    admit_cuts(&mut entry, &sampled.by, &col_ix);
    admit_cuts(&mut exit, &sampled.by, &col_ix);

    CutTable {
        windows,
        entry,
        exit,
        winner_fill: sampled.winner_fill,
        winner_lead: sampled.winner_lead,
        winner_launch: sampled.winner_launch,
    }
}

/// Whether a cut's condition holds for `v` (the one comparison rule search
/// evaluates outside the engine — generator co-occurrence reuses it).
pub(crate) fn cut_holds(op: Operator, threshold: f64, v: f64) -> bool {
    match op {
        Operator::Gt => v > threshold,
        Operator::Gte => v >= threshold,
        Operator::Lt => v < threshold,
        Operator::Lte => v <= threshold,
        Operator::Eq => (v - threshold).abs() < 1e-9,
        Operator::Ne => (v - threshold).abs() >= 1e-9,
    }
}

/// Drop cuts that hold on more than `1 - MIN_REJECT_SHARE` of the cohort's
/// sampled rows. A cut whose column has no / thin samples is kept (no basis to
/// judge it on).
fn admit_cuts(
    cuts: &mut Vec<Cut>,
    by: &HashMap<(usize, Bucket), Vec<f64>>,
    col_ix: &HashMap<(MetricId, i64), usize>,
) {
    cuts.retain(|c| {
        // A ceiling only ever ships inside a band; the floor beside it carries
        // the selectivity gate (a ceil above every row is rare-tail insurance).
        if c.phase == CutPhase::WinnerCeil {
            return true;
        }
        let Some(&ci) = col_ix.get(&fill_key(c.metric, c.window)) else {
            return true;
        };
        let Some(xs) = by.get(&(ci, Bucket::All)) else {
            return true;
        };
        if xs.len() < MIN_SPLIT {
            return true;
        }
        let rejected = xs.iter().filter(|v| !cut_holds(c.op, c.threshold, **v)).count();
        rejected as f64 / xs.len() as f64 >= MIN_REJECT_SHARE
    });
}

fn col_meta(col: SeriesColumn) -> (MetricGroupId, MetricId, Option<hunter_engine::metrics::WindowSpec>) {
    match col {
        SeriesColumn::Static(id) => (group_of(id).id, id, None),
        SeriesColumn::Window(id, w) => (group_of(id).id, id, w.primary),
        SeriesColumn::Fingerprint(id, w, _) => (group_of(id).id, id, w),
    }
}

fn cut_columns(windows: &[f64], with_flow: bool, flow_fp: FingerprintId) -> Vec<SeriesColumn> {
    let fp = if flow_fp.0.is_nil() {
        SWEEP_FLOW_FP
    } else {
        flow_fp
    };
    let mut cols = Vec::new();
    for g in REGISTRY {
        if g.scope == MetricScope::Position {
            continue;
        }
        if matches!(g.family, hunter_engine::metrics::MetricFamily::FlowIx) && !with_flow {
            continue;
        }
        for m in g.metrics {
            match g.kind {
                MetricKind::Static => {
                    cols.push(if is_fingerprint_scoped(m.id) {
                        SeriesColumn::Fingerprint(m.id, None, fp)
                    } else {
                        SeriesColumn::Static(m.id)
                    });
                }
                MetricKind::Dynamic => {
                    for &w in windows {
                        cols.push(if is_fingerprint_scoped(m.id) {
                            SeriesColumn::Fingerprint(m.id, Some(hunter_engine::metrics::WindowSpec::secs(w)), fp)
                        } else {
                            SeriesColumn::window(m.id, hunter_engine::metrics::WindowSpec::secs(w))
                        });
                    }
                }
            }
        }
    }
    cols.sort_by_key(|c| format!("{c:?}"));
    cols.dedup();
    cols
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Bucket {
    Early,
    Peak,
    After,
    AfterDump,
    AfterDumpRun,
    All,
    DumpLead,
    DumpLeadAll,
    WinnerPeak,
    LoserPeak,
    FillWinner,
    FillLoser,
    RunLeadWin,
    RunLeadLose,
    LaunchWin,
    LaunchLose,
    GivebackLead,
    GivebackLeadAll,
}

struct PhaseSamples {
    by: HashMap<(usize, Bucket), Vec<f64>>,
    winner_fill: Vec<HashMap<(MetricId, i64), f64>>,
    winner_lead: Vec<HashMap<(MetricId, i64), f64>>,
    winner_launch: Vec<HashMap<(MetricId, i64), f64>>,
    held_ran: Vec<f64>,
}

fn samples_n(by: &HashMap<(usize, Bucket), Vec<f64>>, bucket: Bucket) -> usize {
    by.iter()
        .filter(|((_, b), _)| *b == bucket)
        .map(|(_, v)| v.len())
        .max()
        .unwrap_or(0)
}

fn label_paths(tokens: &[CorpusToken]) -> Vec<TokenPath> {
    let mut paths: Vec<TokenPath> = tokens
        .iter()
        .map(|t| {
            let (ath_price, ath_at) = token_ath(t);
            let ttp = (ath_at - t.created_at).num_milliseconds() as f64 / 1000.0;
            let first_price = t
                .trades
                .first()
                .map(|tr| tr.price_per_token)
                .filter(|p| p.is_finite() && *p > 0.0)
                .unwrap_or(1.0);
            let t15 = t.trades.iter().find_map(|tr| {
                (tr.price_per_token.is_finite()
                    && first_price > 0.0
                    && tr.price_per_token >= first_price * MIN_ATH_MULT)
                    .then_some(tr.block_time)
            });
            let dump_at = t.trades.iter().find_map(|tr| {
                (tr.block_time > ath_at
                    && tr.price_per_token.is_finite()
                    && ath_price.is_finite()
                    && tr.price_per_token < ath_price * DUMP_FRAC)
                    .then_some(tr.block_time)
            });
            TokenPath {
                ath_price,
                ath_at,
                ttp,
                winner: false,
                dumper: dump_at.is_some(),
                first_price,
                t15,
                dump_at,
            }
        })
        .collect();
    let mut mults: Vec<f64> = paths
        .iter()
        .map(|p| {
            if p.first_price > 0.0 && p.ath_price.is_finite() {
                p.ath_price / p.first_price
            } else {
                1.0
            }
        })
        .filter(|m| m.is_finite())
        .collect();
    let bar = winner_bar(&mut mults);
    for p in &mut paths {
        let m = if p.first_price > 0.0 && p.ath_price.is_finite() {
            p.ath_price / p.first_price
        } else {
            1.0
        };
        p.winner = m >= bar && p.ttp > 0.0;
    }
    paths
}

fn winner_bar(ath_mults: &mut [f64]) -> f64 {
    if ath_mults.len() < MIN_SPLIT * 2 {
        return 2.0;
    }
    ath_mults.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    exact_quantile_f64(ath_mults, 0.67).max(MIN_ATH_MULT)
}

fn sample_phases(
    tokens: &[CorpusToken],
    paths: &[TokenPath],
    columns: &[SeriesColumn],
    flow: Option<&FlowPatterns>,
    flow_fp: FingerprintId,
) -> PhaseSamples {
    let mut by: HashMap<(usize, Bucket), Vec<f64>> = HashMap::new();
    let fp = if flow_fp.0.is_nil() {
        SWEEP_FLOW_FP
    } else {
        flow_fp
    };
    let windows: Vec<hunter_engine::metrics::WindowSpec> = columns
        .iter()
        .filter_map(|c| match c {
            SeriesColumn::Fingerprint(_, w, _) => *w,
            // Single-window by construction here: `cut_columns` only builds
            // `SeriesColumn::window`. A second axis would need its own buffer.
            SeriesColumn::Window(_, w) => w.primary,
            _ => None,
        })
        .collect();
    let lead = Duration::seconds(LEAD_SEC);
    let launch_hi_off = Duration::seconds(LAUNCH_SEC);
    let mut win_fill_secs: Vec<f64> = paths
        .iter()
        .zip(tokens.iter())
        .filter(|(p, _)| p.winner)
        .filter_map(|(p, t)| {
            p.t15.map(|at| (at - t.created_at).num_milliseconds() as f64 / 1000.0)
        })
        .collect();
    let med_fill_secs = median(&mut win_fill_secs).unwrap_or(10.0).max(1.0);
    let mut winner_fill = Vec::new();
    let mut winner_lead = Vec::new();
    let mut winner_launch = Vec::new();
    let mut held_ran = Vec::new();

    for (token, path) in tokens.iter().zip(paths.iter()) {
        if token.trades.is_empty() {
            continue;
        }
        let ttp = path.ttp;
        let peak_lo =
            token.created_at + Duration::milliseconds(((ttp * 0.7) * 1000.0) as i64);
        let peak_hi =
            path.ath_at + Duration::milliseconds(((ttp.abs().max(1.0) * 0.1) * 1000.0) as i64);
        let lead_lo = path.ath_at - lead;
        let fill_at = path.t15.unwrap_or(
            token.created_at + Duration::milliseconds((med_fill_secs * 1000.0) as i64),
        );
        let run_lead_lo = fill_at - lead;
        let launch_hi = token.created_at + launch_hi_off;
        let gb_hi = path.ath_at + lead;

        let mut series = MetricSeries::new(token.created_at, columns.to_vec());
        if let Some(patterns) = flow {
            series.ensure_flow(fp, patterns, &windows);
        }
        for t in token.trades.iter() {
            series.push_trade(to_trade_lite(t));
        }
        let n = series.n_rows();
        let mut snapped = false;
        let mut snapped_launch = false;
        let mut snap: HashMap<(MetricId, i64), f64> = HashMap::new();
        let mut lead_snap: HashMap<(MetricId, i64), f64> = HashMap::new();
        let mut launch_snap: HashMap<(MetricId, i64), f64> = HashMap::new();
        for row in 0..n {
            let at = series.at[row];
            let price = series.price[row];
            let dumped_now = price.is_finite()
                && path.ath_price.is_finite()
                && price < path.ath_price * DUMP_FRAC
                && at > path.ath_at;
            let in_lead = path.dumper && at >= lead_lo && at <= path.ath_at;
            let in_run_lead = at >= run_lead_lo && at < fill_at && at >= token.created_at;
            let in_gb = at > path.ath_at && at <= gb_hi && !dumped_now;
            let at_peak = at <= path.ath_at && at >= peak_lo;
            let at_fill = !snapped && at >= fill_at;
            let at_launch = !snapped_launch && at <= launch_hi;

            for (ci, col) in columns.iter().enumerate() {
                let Some(idx) = series.col_index(*col) else {
                    continue;
                };
                let v = series.value_at(row, idx);
                if !v.is_finite() {
                    continue;
                }
                by.entry((ci, Bucket::All)).or_default().push(v);
                if at_fill {
                    let fill_bucket = if path.winner {
                        Bucket::FillWinner
                    } else {
                        Bucket::FillLoser
                    };
                    by.entry((ci, fill_bucket)).or_default().push(v);
                    if path.winner {
                        let (_, metric, window) = col_meta(*col);
                        snap.insert(fill_key(metric, window), v);
                    }
                }
                if in_run_lead {
                    let b = if path.winner {
                        Bucket::RunLeadWin
                    } else {
                        Bucket::RunLeadLose
                    };
                    by.entry((ci, b)).or_default().push(v);
                    if path.winner {
                        let (_, metric, window) = col_meta(*col);
                        lead_snap.insert(fill_key(metric, window), v);
                    }
                }
                if at_launch {
                    let b = if path.winner {
                        Bucket::LaunchWin
                    } else {
                        Bucket::LaunchLose
                    };
                    by.entry((ci, b)).or_default().push(v);
                    if path.winner {
                        let (_, metric, window) = col_meta(*col);
                        launch_snap.insert(fill_key(metric, window), v);
                    }
                }
                if in_lead {
                    by.entry((ci, Bucket::DumpLeadAll)).or_default().push(v);
                    if path.winner {
                        by.entry((ci, Bucket::DumpLead)).or_default().push(v);
                    }
                }
                if in_gb {
                    by.entry((ci, Bucket::GivebackLeadAll)).or_default().push(v);
                    if path.winner && path.dumper {
                        by.entry((ci, Bucket::GivebackLead)).or_default().push(v);
                    }
                }
                if dumped_now {
                    by.entry((ci, Bucket::AfterDump)).or_default().push(v);
                    if path.winner {
                        by.entry((ci, Bucket::AfterDumpRun)).or_default().push(v);
                    }
                }
                if at_peak {
                    if path.winner {
                        by.entry((ci, Bucket::WinnerPeak)).or_default().push(v);
                    } else {
                        by.entry((ci, Bucket::LoserPeak)).or_default().push(v);
                    }
                    by.entry((ci, Bucket::Peak)).or_default().push(v);
                } else if at <= path.ath_at {
                    by.entry((ci, Bucket::Early)).or_default().push(v);
                } else if at <= peak_hi {
                    by.entry((ci, Bucket::Peak)).or_default().push(v);
                } else if !dumped_now {
                    by.entry((ci, Bucket::After)).or_default().push(v);
                }
            }
            if at_fill {
                snapped = true;
            }
            if at_launch {
                snapped_launch = true;
            }
        }
        if path.winner && !snap.is_empty() {
            winner_fill.push(snap);
        }
        if path.winner && !lead_snap.is_empty() {
            winner_lead.push(lead_snap);
        }
        if path.winner && !launch_snap.is_empty() {
            winner_launch.push(launch_snap);
        }
        if path.winner {
            if let Some(dump_at) = path.dump_at {
                let secs = (dump_at - fill_at).num_milliseconds() as f64 / 1000.0;
                if secs.is_finite() && secs > 0.0 {
                    held_ran.push(secs);
                }
            }
        }
    }
    PhaseSamples {
        by,
        winner_fill,
        winner_lead,
        winner_launch,
        held_ran,
    }
}

fn token_ath(token: &CorpusToken) -> (f64, DateTime<Utc>) {
    let mut best = f64::NEG_INFINITY;
    let mut at = token.created_at;
    for t in token.trades.iter() {
        if t.price_per_token.is_finite() && t.price_per_token > best {
            best = t.price_per_token;
            at = t.block_time;
        }
    }
    (best, at)
}

fn rung(xs: &mut [f64], q: f64, unit: Unit) -> Option<f64> {
    if xs.len() < MIN_SPLIT {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let v = exact_quantile_f64(xs, q);
    if !v.is_finite() {
        return None;
    }
    Some(round_for_unit(v, unit))
}

fn emit(
    out: &mut Vec<Cut>,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    op: Operator,
    threshold: f64,
    phase: CutPhase,
) {
    if !threshold.is_finite() {
        return;
    }
    if out.iter().any(|c| {
        c.group == group
            && c.metric == metric
            && c.window == window
            && c.op == op
            && (c.threshold - threshold).abs() < 1e-9
    }) {
        return;
    }
    out.push(Cut {
        group,
        metric,
        window,
        op,
        threshold,
        phase,
    });
}

fn median_of(by: &HashMap<(usize, Bucket), Vec<f64>>, ci: usize, bucket: Bucket) -> Option<f64> {
    let mut xs = by.get(&(ci, bucket))?.clone();
    if xs.len() < MIN_SPLIT {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(exact_quantile_f64(&xs, 0.5))
}

#[allow(clippy::too_many_arguments)]
fn emit_entry_contrast(
    out: &mut Vec<Cut>,
    by: &HashMap<(usize, Bucket), Vec<f64>>,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    unit: Unit,
    win_b: Bucket,
    lose_b: Bucket,
    phase: CutPhase,
    floor_phase: CutPhase,
) {
    if metric == MetricId::Time {
        if let Some(mut xs) = by.get(&(ci, win_b)).cloned() {
            if let Some(v) = rung(&mut xs, 0.75, unit) {
                emit(out, group, metric, window, Operator::Lt, v, phase);
            }
        }
    } else if let (Some(mw), Some(ml)) = (median_of(by, ci, win_b), median_of(by, ci, lose_b)) {
        let scale = mw.abs().max(ml.abs()).max(1e-9);
        if (mw - ml).abs() / scale >= 0.15 {
            let toward_high = mw > ml;
            let op = if toward_high {
                Operator::Gte
            } else {
                Operator::Lte
            };
            let mid = round_for_unit((mw + ml) * 0.5, unit);
            emit(out, group, metric, window, op, mid, phase);
        }
    }
    if metric != MetricId::Time {
        if let Some(mut xs) = by.get(&(ci, win_b)).cloned() {
            if let Some(v) = rung(&mut xs, 0.10, unit) {
                emit(out, group, metric, window, Operator::Gte, v, floor_phase);
            }
            // p90 ceiling beside the p10 floor — the band pair (one filling).
            if floor_phase == CutPhase::WinnerFloor {
                if let Some(v) = rung(&mut xs, 0.90, unit) {
                    emit(out, group, metric, window, Operator::Lte, v, CutPhase::WinnerCeil);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_entry(
    out: &mut Vec<Cut>,
    by: &HashMap<(usize, Bucket), Vec<f64>>,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    unit: Unit,
    role: EntryRole,
    thick: bool,
    fill_n: usize,
    run_lead_n: usize,
    launch_n: usize,
) {
    if thick {
        // Peak contrast is the primary knife (ATH neighborhood, quieter fill
        // window). Run-lead / launch / fill-moment are extras — they must not
        // replace peak, or the product only times the rip.
        emit_entry_contrast(
            out,
            by,
            ci,
            group,
            metric,
            window,
            unit,
            Bucket::WinnerPeak,
            Bucket::LoserPeak,
            CutPhase::Contrast,
            CutPhase::WinnerFloor,
        );
        if run_lead_n >= MIN_SPLIT {
            emit_entry_contrast(
                out,
                by,
                ci,
                group,
                metric,
                window,
                unit,
                Bucket::RunLeadWin,
                Bucket::RunLeadLose,
                CutPhase::RunLead,
                CutPhase::RunLead,
            );
        }
        if launch_n >= MIN_SPLIT {
            emit_entry_contrast(
                out,
                by,
                ci,
                group,
                metric,
                window,
                unit,
                Bucket::LaunchWin,
                Bucket::LaunchLose,
                CutPhase::Launch,
                CutPhase::Launch,
            );
        }
        if fill_n >= MIN_SPLIT {
            emit_entry_contrast(
                out,
                by,
                ci,
                group,
                metric,
                window,
                unit,
                Bucket::FillWinner,
                Bucket::FillLoser,
                CutPhase::FillMoment,
                CutPhase::FillMoment,
            );
        }
        return;
    }

    let op_primary = match role {
        EntryRole::Selector if metric == MetricId::Time => Operator::Lt,
        EntryRole::Selector if metric == MetricId::Liquidity => Operator::Gte,
        EntryRole::Selector => Operator::Gte,
        EntryRole::Extra | EntryRole::Trigger(_) => Operator::Gte,
        EntryRole::WaitOnly => return,
    };
    for (bucket, phase, q, op) in [
        (Bucket::Early, CutPhase::Early, 0.50, op_primary),
        (Bucket::Peak, CutPhase::Peak, 0.50, op_primary),
    ] {
        if let Some(mut xs) = by.get(&(ci, bucket)).cloned() {
            if let Some(v) = rung(&mut xs, q, unit) {
                emit(out, group, metric, window, op, v, phase);
            }
        }
    }
    if metric == MetricId::Time {
        if let Some(mut xs) = by.get(&(ci, Bucket::Peak)).cloned() {
            if let Some(v) = rung(&mut xs, 0.75, unit) {
                emit(out, group, metric, window, Operator::Lt, v, CutPhase::Peak);
            }
        }
    }
    if metric == MetricId::Liquidity {
        if let Some(mut xs) = by.get(&(ci, Bucket::Peak)).cloned() {
            if let Some(v) = rung(&mut xs, 0.75, unit) {
                emit(out, group, metric, window, Operator::Lte, v, CutPhase::Peak);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_exit_rungs(
    out: &mut Vec<Cut>,
    by: &HashMap<(usize, Bucket), Vec<f64>>,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    unit: Unit,
    bucket: Bucket,
    phase: CutPhase,
) {
    if let Some(mut xs) = by.get(&(ci, bucket)).cloned() {
        if let Some(v) = rung(&mut xs, 0.50, unit) {
            emit(out, group, metric, window, Operator::Gte, v, phase);
        }
        if let Some(v) = rung(&mut xs, 0.90, unit) {
            emit(out, group, metric, window, Operator::Gte, v, phase);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_exit(
    out: &mut Vec<Cut>,
    by: &HashMap<(usize, Bucket), Vec<f64>>,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<hunter_engine::metrics::WindowSpec>,
    unit: Unit,
    lead_n: usize,
    dump_run_n: usize,
    gb_n: usize,
) {
    let lead_bucket = if lead_n >= MIN_SPLIT {
        Bucket::DumpLead
    } else {
        Bucket::DumpLeadAll
    };
    let dump_bucket = if dump_run_n >= MIN_SPLIT {
        Bucket::AfterDumpRun
    } else {
        Bucket::AfterDump
    };
    let gb_bucket = if gb_n >= MIN_SPLIT {
        Bucket::GivebackLead
    } else {
        Bucket::GivebackLeadAll
    };
    emit_exit_rungs(out, by, ci, group, metric, window, unit, lead_bucket, CutPhase::DumpLead);
    emit_exit_rungs(out, by, ci, group, metric, window, unit, gb_bucket, CutPhase::GivebackLead);
    if let Some(mut xs) = by.get(&(ci, dump_bucket)).cloned() {
        if let Some(v) = rung(&mut xs, 0.50, unit) {
            emit(
                out,
                group,
                metric,
                window,
                Operator::Gte,
                v,
                CutPhase::AfterDump,
            );
        }
        if let Some(v) = rung(&mut xs, 0.90, unit) {
            emit(
                out,
                group,
                metric,
                window,
                Operator::Gte,
                v,
                CutPhase::DumperP90,
            );
        }
    }
}

fn cohort_windows(tokens: &[CorpusToken], paths: &[TokenPath]) -> Vec<f64> {
    let mut ttp_all = Vec::new();
    let mut ttp_win = Vec::new();
    let mut ttp_lose = Vec::new();
    let mut spacing = Vec::new();
    for (t, p) in tokens.iter().zip(paths.iter()) {
        if t.trades.len() < 2 {
            continue;
        }
        if p.ttp.is_finite() && p.ttp > 0.0 {
            ttp_all.push(p.ttp);
            if p.winner {
                ttp_win.push(p.ttp);
            } else {
                ttp_lose.push(p.ttp);
            }
        }
        let mut prev = t.trades[0].block_time;
        for tr in t.trades.iter().skip(1) {
            let dt = (tr.block_time - prev).num_milliseconds() as f64 / 1000.0;
            if dt.is_finite() && dt > 0.0 {
                spacing.push(dt);
            }
            prev = tr.block_time;
        }
    }
    let med_space = median(&mut spacing).unwrap_or(3.0).clamp(1.0, 30.0);
    let med_ttp = median(&mut ttp_all).unwrap_or(60.0).clamp(med_space, 600.0);
    let med_win = median(&mut ttp_win).unwrap_or(med_ttp).clamp(med_space, 600.0);
    let med_lose = median(&mut ttp_lose).unwrap_or(med_ttp).clamp(med_space, 600.0);
    let short = round_for_unit(med_space.max(2.0), Unit::Seconds);
    let burst = round_for_unit((0.25 * med_win).clamp(short, 120.0), Unit::Seconds);
    let near = round_for_unit((0.6 * med_ttp).clamp(burst, 300.0), Unit::Seconds);
    let grind = round_for_unit((0.25 * med_lose).clamp(short, 180.0), Unit::Seconds);
    let mut w = vec![short, burst, near];
    if (grind - burst).abs() >= 1.0 && (grind - near).abs() >= 1.0 {
        w.push(grind);
    }
    w.retain(|x| *x > 0.0 && x.is_finite());
    w.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    w.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    if w.len() > 4 {
        w.truncate(4);
    }
    if w.is_empty() {
        w = vec![3.0, 10.0, 30.0];
    }
    w
}

fn push_outcome_held(out: &mut Vec<Cut>, held_ran: &[f64]) {
    if held_ran.len() < MIN_SPLIT {
        return;
    }
    let mut xs = held_ran.to_vec();
    let group = group_of(MetricId::Held).id;
    for q in [0.50, 0.75] {
        if let Some(v) = rung(&mut xs, q, Unit::Seconds) {
            if v >= 2.0 {
                emit(
                    out,
                    group,
                    MetricId::Held,
                    None,
                    Operator::Gte,
                    v,
                    CutPhase::Outcome,
                );
            }
        }
    }
}

fn median(xs: &mut [f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(exact_quantile_f64(xs, 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::fixtures::{self, created_at, trade};
    use std::sync::Arc;

    fn path_token(
        mint: &str,
        n: usize,
        peak_mult: f64,
        reserve: f64,
        lead_buy: f64,
    ) -> CorpusToken {
        path_token_peak(mint, n, n / 2, peak_mult, reserve, lead_buy)
    }

    fn path_token_peak(
        mint: &str,
        n: usize,
        peak_i: usize,
        peak_mult: f64,
        reserve: f64,
        lead_buy: f64,
    ) -> CorpusToken {
        let peak_i = peak_i.max(1).min(n.saturating_sub(1));
        let trades: Vec<_> = (0..n)
            .map(|i| {
                let price = if i <= peak_i {
                    1.0 + (peak_mult - 1.0) * (i as f64 / peak_i as f64)
                } else {
                    let fall = (i - peak_i) as f64 / (n - 1 - peak_i) as f64;
                    peak_mult * (1.0 - 0.5 * fall)
                };
                let mut t = trade(i as i64, price, reserve, true);
                if i + LEAD_SEC as usize >= peak_i && i <= peak_i {
                    t.amount_sol = lead_buy;
                } else {
                    t.amount_sol = 0.4;
                }
                t
            })
            .collect();
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: created_at(),
            trades: Arc::new(trades),
            fp: Default::default(),
            identity: None,
            peak_after: None,
        }
    }

    fn contrast_corpus() -> Vec<CorpusToken> {
        let mut tokens = Vec::new();
        for i in 0..10 {
            tokens.push(path_token(&format!("w{i}"), 24, 4.0, 80.0, 8.0));
        }
        for i in 0..20 {
            tokens.push(path_token(&format!("l{i}"), 24, 1.2, 15.0, 0.4));
        }
        tokens
    }

    fn slow_run_corpus() -> Vec<CorpusToken> {
        let mut tokens = Vec::new();
        for i in 0..10 {
            tokens.push(path_token_peak(&format!("w{i}"), 40, 30, 4.0, 80.0, 8.0));
        }
        for i in 0..20 {
            tokens.push(path_token_peak(&format!("l{i}"), 40, 30, 1.2, 15.0, 0.4));
        }
        tokens
    }

    #[test]
    fn windows_come_from_cohort_clocks() {
        let tok = fixtures::token("m0", 24);
        let paths = label_paths(std::slice::from_ref(&tok));
        let w = cohort_windows(std::slice::from_ref(&tok), &paths);
        assert!(!w.is_empty());
        assert!(w.iter().all(|x| *x >= 1.0));
        assert!(w.len() <= 4);
    }

    #[test]
    fn empty_corpus_is_empty_table() {
        let t = build_cut_table(&[], None, FingerprintId(uuid::Uuid::nil()));
        assert!(t.entry.is_empty());
        assert!(t.exit.is_empty());
    }

    #[test]
    fn admission_drops_a_cut_that_rejects_nothing() {
        // `>= 0` on a non-negative metric holds on every sampled row — vacuous.
        let mut by: HashMap<(usize, Bucket), Vec<f64>> = HashMap::new();
        by.insert((0, Bucket::All), (0..40).map(|i| i as f64).collect());
        let mut col_ix = HashMap::new();
        col_ix.insert(fill_key(MetricId::Liquidity, None), 0usize);
        let group = group_of(MetricId::Liquidity).id;
        let mut cuts = vec![
            Cut {
                group,
                metric: MetricId::Liquidity,
                window: None,
                op: Operator::Gte,
                threshold: 0.0,
                phase: CutPhase::WinnerFloor,
            },
            Cut {
                group,
                metric: MetricId::Liquidity,
                window: None,
                op: Operator::Gte,
                threshold: 20.0,
                phase: CutPhase::Contrast,
            },
        ];
        admit_cuts(&mut cuts, &by, &col_ix);
        assert_eq!(cuts.len(), 1, "vacuous >= 0 floor must be dropped");
        assert!((cuts[0].threshold - 20.0).abs() < 1e-9);
    }

    #[test]
    fn admission_keeps_cuts_with_no_sample_basis() {
        let by: HashMap<(usize, Bucket), Vec<f64>> = HashMap::new();
        let col_ix = HashMap::new();
        let group = group_of(MetricId::Retrace).id;
        let mut cuts = vec![Cut {
            group,
            metric: MetricId::Retrace,
            window: None,
            op: Operator::Gte,
            threshold: 10.0,
            phase: CutPhase::Declared,
        }];
        admit_cuts(&mut cuts, &by, &col_ix);
        assert_eq!(cuts.len(), 1, "no sampled column ⇒ kept");
    }

    #[test]
    fn winner_ceil_pairs_the_floor_on_thick_splits() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let liq: Vec<&Cut> = t
            .entry
            .iter()
            .filter(|c| c.metric == MetricId::Liquidity)
            .collect();
        let floor = liq.iter().find(|c| c.phase == CutPhase::WinnerFloor);
        let ceil = liq.iter().find(|c| c.phase == CutPhase::WinnerCeil);
        assert!(floor.is_some(), "winner floor expected");
        let (floor, ceil) = (floor.unwrap(), ceil.expect("winner ceil beside the floor"));
        assert_eq!(ceil.op, Operator::Lte);
        assert!(
            ceil.threshold >= floor.threshold,
            "ceil {} must sit at or above floor {}",
            ceil.threshold,
            floor.threshold
        );
    }

    #[test]
    fn position_exits_are_declared() {
        let corpus = fixtures::corpus(4);
        let t = build_cut_table(&corpus.tokens, None, FingerprintId(uuid::Uuid::nil()));
        assert!(t.exit.iter().any(|c| c.metric == MetricId::Retrace && c.phase == CutPhase::Declared));
        assert!(t.exit.iter().any(|c| c.metric == MetricId::Held));
        assert!(t.entry.iter().all(|c| !is_position(c.metric)));
    }

    #[test]
    fn contrast_lands_in_the_liq_gap_not_with_the_loser_majority() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let liq: Vec<&Cut> = t
            .entry
            .iter()
            .filter(|c| c.metric == MetricId::Liquidity)
            .collect();
        assert!(
            liq.iter().any(|c| c.phase == CutPhase::Contrast),
            "expected peak contrast liq, got {:?}",
            liq.iter().map(|c| (c.phase, c.op, c.threshold)).collect::<Vec<_>>()
        );
        let contrast = liq
            .iter()
            .find(|c| c.phase == CutPhase::Contrast)
            .expect("contrast");
        // Ran peak ~80, never-ran ~15, midpoint ~47. Mixed p50 of 10+20 tokens sits
        // with the losers (~15). The knife must not sit there.
        assert!(
            contrast.threshold > 25.0 && contrast.threshold < 70.0,
            "contrast liq {} not in the gap",
            contrast.threshold
        );
        assert_eq!(contrast.op, Operator::Gte);
        assert!(liq.iter().any(|c| c.phase == CutPhase::WinnerFloor && c.threshold >= 50.0));
    }

    #[test]
    fn dump_lead_buy_is_higher_than_runner_after_dump() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let buy_lead: Vec<&Cut> = t
            .exit
            .iter()
            .filter(|c| c.metric == MetricId::Buy && c.phase == CutPhase::DumpLead)
            .collect();
        assert!(
            !buy_lead.is_empty(),
            "expected dump-lead buy cuts, exit phases {:?}",
            t.exit
                .iter()
                .filter(|c| c.metric == MetricId::Buy)
                .map(|c| (c.phase, c.threshold, c.window))
                .collect::<Vec<_>>()
        );
        // Ran dumpers spike amount_sol=8 in the lead; never-ran stay at 0.4.
        assert!(
            buy_lead.iter().any(|c| c.threshold >= 3.0),
            "dump-lead buy too low: {:?}",
            buy_lead.iter().map(|c| c.threshold).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fill_moment_time_cap_is_before_peak() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let time: Vec<&Cut> = t.entry.iter().filter(|c| c.metric == MetricId::Time).collect();
        let fill = time
            .iter()
            .find(|c| c.phase == CutPhase::FillMoment)
            .expect("fill-moment time");
        let peak = time
            .iter()
            .find(|c| c.phase == CutPhase::Contrast)
            .expect("peak contrast time");
        // 1.5× lands near t=2 on the winner path; TTP is ~12. Fill-moment is an
        // extra; peak contrast must still be on the table.
        assert!(
            fill.threshold < 8.0,
            "fill-moment time cap {} is not a time-to-1.5× knife",
            fill.threshold
        );
        assert!(
            peak.threshold > fill.threshold,
            "peak time {} should be later than fill-moment {}",
            peak.threshold,
            fill.threshold
        );
        assert_eq!(fill.op, Operator::Lt);
        assert_eq!(peak.op, Operator::Lt);
    }

    #[test]
    fn thick_split_keeps_peak_when_fill_moment_exists() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let time: Vec<&Cut> = t.entry.iter().filter(|c| c.metric == MetricId::Time).collect();
        assert!(
            time.iter().any(|c| c.phase == CutPhase::Contrast),
            "peak contrast time must stay, got {:?}",
            time.iter().map(|c| (c.phase, c.threshold)).collect::<Vec<_>>()
        );
        assert!(
            time.iter().any(|c| c.phase == CutPhase::FillMoment),
            "fill-moment time is an extra when the clocks differ, got {:?}",
            time.iter().map(|c| (c.phase, c.threshold)).collect::<Vec<_>>()
        );
        let liq: Vec<&Cut> = t
            .entry
            .iter()
            .filter(|c| c.metric == MetricId::Liquidity)
            .collect();
        assert!(
            liq.iter().any(|c| c.phase == CutPhase::Contrast),
            "peak contrast liq must stay, got {:?}",
            liq.iter().map(|c| (c.phase, c.threshold)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn held_comes_from_fill_to_dump_not_declared_minutes() {
        let tokens = contrast_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let held: Vec<&Cut> = t.exit.iter().filter(|c| c.metric == MetricId::Held).collect();
        assert!(
            held.iter().any(|c| c.phase == CutPhase::Outcome && c.threshold < 60.0),
            "expected outcome held from a ~15s dump, got {:?}",
            held.iter().map(|c| (c.phase, c.threshold)).collect::<Vec<_>>()
        );
        assert!(held.iter().all(|c| c.phase != CutPhase::Declared || c.threshold < 60.0));
    }

    #[test]
    fn run_lead_time_is_before_fill_moment() {
        let tokens = slow_run_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let time: Vec<&Cut> = t.entry.iter().filter(|c| c.metric == MetricId::Time).collect();
        let lead = time
            .iter()
            .find(|c| c.phase == CutPhase::RunLead)
            .expect("run-lead time");
        let fill = time
            .iter()
            .find(|c| c.phase == CutPhase::FillMoment)
            .expect("fill-moment time");
        let peak = time
            .iter()
            .find(|c| c.phase == CutPhase::Contrast)
            .expect("peak contrast time");
        assert!(
            lead.threshold < fill.threshold,
            "run-lead {} should be before fill-moment {}",
            lead.threshold,
            fill.threshold
        );
        assert!(
            fill.threshold < peak.threshold,
            "fill-moment {} should be before peak {}",
            fill.threshold,
            peak.threshold
        );
        assert_eq!(lead.op, Operator::Lt);
    }

    #[test]
    fn launch_time_exists_beside_peak() {
        let tokens = slow_run_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let time: Vec<&Cut> = t.entry.iter().filter(|c| c.metric == MetricId::Time).collect();
        assert!(
            time.iter().any(|c| c.phase == CutPhase::Launch),
            "launch time must be an extra, got {:?}",
            time.iter().map(|c| (c.phase, c.threshold)).collect::<Vec<_>>()
        );
        assert!(time.iter().any(|c| c.phase == CutPhase::Contrast));
    }

    #[test]
    fn giveback_lead_is_after_ath_not_dump_lead() {
        let tokens = slow_run_corpus();
        let t = build_cut_table(&tokens, None, FingerprintId(uuid::Uuid::nil()));
        let buy_dump: Vec<&Cut> = t
            .exit
            .iter()
            .filter(|c| c.metric == MetricId::Buy && c.phase == CutPhase::DumpLead)
            .collect();
        let buy_gb: Vec<&Cut> = t
            .exit
            .iter()
            .filter(|c| c.metric == MetricId::Buy && c.phase == CutPhase::GivebackLead)
            .collect();
        assert!(
            !buy_dump.is_empty(),
            "dump-lead buy must stay"
        );
        assert!(
            !buy_gb.is_empty(),
            "giveback-lead buy must exist, exit phases {:?}",
            t.exit
                .iter()
                .filter(|c| c.metric == MetricId::Buy)
                .map(|c| (c.phase, c.threshold))
                .collect::<Vec<_>>()
        );
        let dump_hi = buy_dump.iter().map(|c| c.threshold).fold(0.0, f64::max);
        let gb_hi = buy_gb.iter().map(|c| c.threshold).fold(0.0, f64::max);
        assert!(
            dump_hi > gb_hi,
            "dump-lead buy {dump_hi} should spike above giveback-lead {gb_hi}"
        );
    }
}
