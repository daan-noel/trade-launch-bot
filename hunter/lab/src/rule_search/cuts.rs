//! Cut table from this range's metric paths ([rule-search-method.md] §2).
//!
//! Labels are path facts (ran vs never-ran, dumpers), not a rule. Entry uses
//! contrast + winner floor when the split is thick; mixed peak otherwise. Exit
//! uses dump-lead, runner after-dump, and dumper p90. Position-scoped metrics
//! have no token path — they use a declared menu.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_of, is_flow_metric, metric_spec, MetricGroupId, MetricId, MetricKind, MetricScope,
    Unit, REGISTRY,
};
use trading_core::strategies::kernel::exact_quantile_f64;

use crate::discovery::candidates::round_for_unit;
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::axes::SWEEP_FLOW_FP;
use crate::sweep::projection::to_trade_lite;

use super::roles::{entry_role, is_position, EntryRole};

/// Seconds before ATH that count as dump-lead.
const DUMP_LEAD_SEC: i64 = 3;
/// Minimum tokens on each side before contrast / dump-lead prefer the split.
const MIN_SPLIT: usize = 8;
const DUMP_FRAC: f64 = 0.85;
const MIN_ATH_MULT: f64 = 1.5;

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
    DumperP90,
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
            Self::DumperP90 => "dumper-p90",
        }
    }

    /// Lower is kept first when the generator truncates the menu.
    pub fn menu_rank(self) -> u8 {
        match self {
            Self::Contrast => 0,
            Self::WinnerFloor => 1,
            Self::DumpLead => 2,
            Self::DumperP90 | Self::AfterDump => 3,
            Self::Peak => 4,
            Self::Early => 5,
            Self::After | Self::P90 => 6,
            Self::Declared => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub group: MetricGroupId,
    pub metric: MetricId,
    pub window: Option<f64>,
    pub op: Operator,
    pub threshold: f64,
    pub phase: CutPhase,
}

#[derive(Clone, Debug)]
pub struct CutTable {
    pub windows: Vec<f64>,
    pub entry: Vec<Cut>,
    pub exit: Vec<Cut>,
}

impl CutTable {
    pub fn empty() -> Self {
        Self {
            windows: Vec::new(),
            entry: Vec::new(),
            exit: Vec::new(),
        }
    }
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
    let samples = sample_phases(tokens, &paths, &columns, flow, flow_fp);
    let thick = paths.iter().filter(|p| p.winner).count() >= MIN_SPLIT
        && paths.iter().filter(|p| !p.winner).count() >= MIN_SPLIT;
    let lead_n = samples_n(&samples, Bucket::DumpLead);
    let dump_run_n = samples_n(&samples, Bucket::AfterDumpRun);

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
                    &samples,
                    ci,
                    group,
                    metric,
                    window,
                    unit,
                    role,
                    thick,
                );
            }
        }
        push_exit(
            &mut exit,
            &samples,
            ci,
            group,
            metric,
            window,
            unit,
            lead_n,
            dump_run_n,
        );
    }
    for &(id, op, vals) in POSITION_EXIT {
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
    CutTable {
        windows,
        entry,
        exit,
    }
}

fn col_meta(col: SeriesColumn) -> (MetricGroupId, MetricId, Option<f64>) {
    match col {
        SeriesColumn::Static(id) => (group_of(id).id, id, None),
        SeriesColumn::Window(id, w) => (group_of(id).id, id, Some(w)),
        SeriesColumn::Flow(id, w, _) => (group_of(id).id, id, w),
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
        if matches!(g.family, hunter_engine::metrics::MetricFamily::FlowSplit) && !with_flow {
            continue;
        }
        for m in g.metrics {
            match g.kind {
                MetricKind::Static => {
                    cols.push(if is_flow_metric(m.id) {
                        SeriesColumn::Flow(m.id, None, fp)
                    } else {
                        SeriesColumn::Static(m.id)
                    });
                }
                MetricKind::Dynamic => {
                    for &w in windows {
                        cols.push(if is_flow_metric(m.id) {
                            SeriesColumn::Flow(m.id, Some(w), fp)
                        } else {
                            SeriesColumn::Window(m.id, w)
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
}

struct PhaseSamples {
    by: HashMap<(usize, Bucket), Vec<f64>>,
}

fn samples_n(samples: &PhaseSamples, bucket: Bucket) -> usize {
    samples
        .by
        .iter()
        .filter(|((_, b), _)| *b == bucket)
        .map(|(_, v)| v.len())
        .max()
        .unwrap_or(0)
}

fn label_paths(tokens: &[CorpusToken]) -> Vec<TokenPath> {
    let unpacked: Vec<(f64, DateTime<Utc>, f64, bool, f64)> = tokens
        .iter()
        .map(|t| {
            let (ath_price, ath_at) = token_ath(t);
            let ttp = (ath_at - t.created_at).num_milliseconds() as f64 / 1000.0;
            let first = t
                .trades
                .first()
                .map(|tr| tr.price_per_token)
                .filter(|p| p.is_finite() && *p > 0.0)
                .unwrap_or(1.0);
            let ath_mult = if first > 0.0 && ath_price.is_finite() {
                ath_price / first
            } else {
                1.0
            };
            let dumper = t.trades.iter().any(|tr| {
                tr.block_time > ath_at
                    && tr.price_per_token.is_finite()
                    && ath_price.is_finite()
                    && tr.price_per_token < ath_price * DUMP_FRAC
            });
            (ath_price, ath_at, ttp, dumper, ath_mult)
        })
        .collect();
    let mut ath_mults: Vec<f64> = unpacked
        .iter()
        .filter(|r| r.4.is_finite())
        .map(|r| r.4)
        .collect();
    let bar = winner_bar(&mut ath_mults);
    unpacked
        .into_iter()
        .map(|(ath_price, ath_at, ttp, dumper, ath_mult)| TokenPath {
            ath_price,
            ath_at,
            ttp,
            winner: ath_mult >= bar && ttp > 0.0,
            dumper,
        })
        .collect()
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
    let windows: Vec<f64> = columns
        .iter()
        .filter_map(|c| match c {
            SeriesColumn::Flow(_, Some(w), _) | SeriesColumn::Window(_, w) => Some(*w),
            _ => None,
        })
        .collect();
    let lead = Duration::seconds(DUMP_LEAD_SEC);

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

        let mut series = MetricSeries::new(token.created_at, columns.to_vec());
        if let Some(patterns) = flow {
            series.ensure_flow(fp, patterns, &windows);
        }
        for t in token.trades.iter() {
            series.push_trade(to_trade_lite(t));
        }
        let n = series.n_rows();
        for row in 0..n {
            let at = series.at[row];
            let price = series.price[row];
            let dumped_now = price.is_finite()
                && path.ath_price.is_finite()
                && price < path.ath_price * DUMP_FRAC
                && at > path.ath_at;
            let in_lead = path.dumper && at >= lead_lo && at <= path.ath_at;
            let at_peak = at <= path.ath_at && at >= peak_lo;

            for (ci, col) in columns.iter().enumerate() {
                let Some(idx) = series.col_index(*col) else {
                    continue;
                };
                let v = series.value_at(row, idx);
                if !v.is_finite() {
                    continue;
                }
                by.entry((ci, Bucket::All)).or_default().push(v);
                if in_lead {
                    by.entry((ci, Bucket::DumpLeadAll)).or_default().push(v);
                    if path.winner {
                        by.entry((ci, Bucket::DumpLead)).or_default().push(v);
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
        }
    }
    PhaseSamples { by }
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
    window: Option<f64>,
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

fn median_of(samples: &PhaseSamples, ci: usize, bucket: Bucket) -> Option<f64> {
    let mut xs = samples.by.get(&(ci, bucket))?.clone();
    if xs.len() < MIN_SPLIT {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(exact_quantile_f64(&xs, 0.5))
}

#[allow(clippy::too_many_arguments)]
fn push_entry(
    out: &mut Vec<Cut>,
    samples: &PhaseSamples,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<f64>,
    unit: Unit,
    role: EntryRole,
    thick: bool,
) {
    if thick {
        if metric == MetricId::Time {
            if let Some(mut xs) = samples.by.get(&(ci, Bucket::WinnerPeak)).cloned() {
                if let Some(v) = rung(&mut xs, 0.75, unit) {
                    emit(out, group, metric, window, Operator::Lt, v, CutPhase::Contrast);
                }
            }
        } else if let (Some(mw), Some(ml)) = (
            median_of(samples, ci, Bucket::WinnerPeak),
            median_of(samples, ci, Bucket::LoserPeak),
        ) {
            let scale = mw.abs().max(ml.abs()).max(1e-9);
            if (mw - ml).abs() / scale >= 0.15 {
                let toward_high = mw > ml;
                let op = if toward_high {
                    Operator::Gte
                } else {
                    Operator::Lte
                };
                let mid = round_for_unit((mw + ml) * 0.5, unit);
                emit(out, group, metric, window, op, mid, CutPhase::Contrast);
            }
        }
        if metric != MetricId::Time {
            if let Some(mut xs) = samples.by.get(&(ci, Bucket::WinnerPeak)).cloned() {
                if let Some(v) = rung(&mut xs, 0.10, unit) {
                    emit(
                        out,
                        group,
                        metric,
                        window,
                        Operator::Gte,
                        v,
                        CutPhase::WinnerFloor,
                    );
                }
            }
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
        if let Some(mut xs) = samples.by.get(&(ci, bucket)).cloned() {
            if let Some(v) = rung(&mut xs, q, unit) {
                emit(out, group, metric, window, op, v, phase);
            }
        }
    }
    if metric == MetricId::Time {
        if let Some(mut xs) = samples.by.get(&(ci, Bucket::Peak)).cloned() {
            if let Some(v) = rung(&mut xs, 0.75, unit) {
                emit(out, group, metric, window, Operator::Lt, v, CutPhase::Peak);
            }
        }
    }
    if metric == MetricId::Liquidity {
        if let Some(mut xs) = samples.by.get(&(ci, Bucket::Peak)).cloned() {
            if let Some(v) = rung(&mut xs, 0.75, unit) {
                emit(out, group, metric, window, Operator::Lte, v, CutPhase::Peak);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_exit(
    out: &mut Vec<Cut>,
    samples: &PhaseSamples,
    ci: usize,
    group: MetricGroupId,
    metric: MetricId,
    window: Option<f64>,
    unit: Unit,
    lead_n: usize,
    dump_run_n: usize,
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
    if let Some(mut xs) = samples.by.get(&(ci, lead_bucket)).cloned() {
        if let Some(v) = rung(&mut xs, 0.50, unit) {
            emit(
                out,
                group,
                metric,
                window,
                Operator::Gte,
                v,
                CutPhase::DumpLead,
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
                CutPhase::DumpLead,
            );
        }
    }
    if let Some(mut xs) = samples.by.get(&(ci, dump_bucket)).cloned() {
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
    let mut spacing = Vec::new();
    for (t, p) in tokens.iter().zip(paths.iter()) {
        if t.trades.len() < 2 {
            continue;
        }
        if p.ttp.is_finite() && p.ttp > 0.0 {
            ttp_all.push(p.ttp);
            if p.winner {
                ttp_win.push(p.ttp);
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
    let short = round_for_unit(med_space.max(2.0), Unit::Seconds);
    let burst = round_for_unit((0.25 * med_win).clamp(short, 120.0), Unit::Seconds);
    let near = round_for_unit((0.6 * med_ttp).clamp(burst, 300.0), Unit::Seconds);
    let mut w = vec![short, burst, near];
    w.retain(|x| *x > 0.0 && x.is_finite());
    w.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    w.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    if w.is_empty() {
        w = vec![3.0, 10.0, 30.0];
    }
    w
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
        let peak_i = n / 2;
        let trades: Vec<_> = (0..n)
            .map(|i| {
                let price = if i <= peak_i {
                    1.0 + (peak_mult - 1.0) * (i as f64 / peak_i as f64)
                } else {
                    let fall = (i - peak_i) as f64 / (n - 1 - peak_i) as f64;
                    peak_mult * (1.0 - 0.5 * fall)
                };
                let mut t = trade(i as i64, price, reserve, true);
                if i + DUMP_LEAD_SEC as usize >= peak_i && i <= peak_i {
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
            "expected contrast liq, got {:?}",
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
        // Losers spike amount_sol=8 in the lead; winners stay at 0.4. Mixed
        // after-dump smears both. Dump-lead must sit with the spike.
        assert!(
            buy_lead.iter().any(|c| c.threshold >= 3.0),
            "dump-lead buy too low: {:?}",
            buy_lead.iter().map(|c| c.threshold).collect::<Vec<_>>()
        );
    }
}
