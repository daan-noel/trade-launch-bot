//! Fast archive: `CompiledRule` series walk, shared entry across exit bags,
//! then a global time-order merge for copycat (and caps when the run has them).
//!
//! Horizon is Simulate's (`as_of` / corpus last trade via [`frozen_tail_horizon`]),
//! not sweep's per-token tail cap as the PnL authority. Report columns still come
//! from `run_replay`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Duration, Utc};
use rayon::prelude::*;
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::grid::SparseGrid;
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::metrics::{group_spec, MetricId, MetricKind, MetricScope};
use trading_core::config::constants::sol_to_lamports;

use crate::sweep::aggregate::ComboAgg;
use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::axes::SWEEP_FLOW_FP;
use crate::sweep::generic::strategy::{
    build_series_with_flow, entry_candidates, frozen_tail_horizon, resolve_entry_from, resolve_exit,
    BoundCombo, EntryCandidates, Pricing,
};
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategy::TokenOutcome;

use super::generator::{Clause, GeneratedCombo};
use hunter_engine::metrics::flow_ix::FlowPatterns;

/// Time blocks a range is split into for the robust (trimmed) rank and the
/// per-quartile PnL row.
pub const N_BLOCKS: usize = 4;

/// One combo's slim archive row (ranking only — not a Simulate persist).
#[derive(Clone, Debug)]
pub struct ArchiveRow {
    pub combo_idx: usize,
    pub n_fired: u64,
    pub n_closed: u64,
    /// Distinct tokens entered after guards — the effective n (trades cluster).
    pub n_tokens: u64,
    pub n_fired_unguarded: u64,
    pub total_pnl_sol: f64,
    /// Realized SOL per time quartile of the search range (entry-time bucketed).
    pub block_pnl_sol: [f64; N_BLOCKS],
    pub profit_factor: Option<f64>,
    pub win_rate: f64,
}

impl ArchiveRow {
    pub fn enter_pct(&self, n_matched: u64) -> f64 {
        if n_matched == 0 {
            0.0
        } else {
            self.n_fired_unguarded as f64 / n_matched as f64
        }
    }

    /// Trimmed rank key: total minus the best block (the sum of the other
    /// blocks), so an edge that lives entirely in one launch burst cannot
    /// outrank one that pays across the range.
    pub fn robust_sol(&self) -> f64 {
        let best = self
            .block_pnl_sol
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if best.is_finite() {
            self.total_pnl_sol - best.max(0.0)
        } else {
            self.total_pnl_sol
        }
    }

    pub fn pays(&self, min_tokens: u64, expectancy_floor_sol: f64) -> bool {
        self.n_tokens >= min_tokens
            && self.total_pnl_sol > 0.0
            && self.profit_factor.map(|p| p > 1.0).unwrap_or(true)
            && (self.n_closed == 0
                || self.total_pnl_sol / self.n_closed as f64 >= expectancy_floor_sol)
    }
}

/// `[min created_at, max created_at]` of the corpus — the range block buckets
/// split. One definition for the fast archive and the replay report.
pub fn corpus_range(created: impl Iterator<Item = DateTime<Utc>>) -> (DateTime<Utc>, DateTime<Utc>) {
    let mut lo: Option<DateTime<Utc>> = None;
    let mut hi: Option<DateTime<Utc>> = None;
    for at in created {
        lo = Some(lo.map_or(at, |v| v.min(at)));
        hi = Some(hi.map_or(at, |v| v.max(at)));
    }
    let lo = lo.unwrap_or_else(Utc::now);
    (lo, hi.unwrap_or(lo))
}

/// Which of the `N_BLOCKS` equal time blocks of `[lo, hi]` holds `at` (clamped).
pub fn block_of(at: DateTime<Utc>, range: (DateTime<Utc>, DateTime<Utc>)) -> usize {
    let (lo, hi) = range;
    let span_ms = (hi - lo).num_milliseconds();
    if span_ms <= 0 {
        return 0;
    }
    let off_ms = (at - lo).num_milliseconds().clamp(0, span_ms);
    ((off_ms * N_BLOCKS as i64) / (span_ms + 1)) as usize
}

pub struct ScoreConfig<'a> {
    pub pricing: Pricing,
    pub as_of: DateTime<Utc>,
    pub flow: Option<&'a FlowPatterns>,
    pub flow_fp: FingerprintId,
    pub skip_duplicate_identity: bool,
    pub duplicate_identity_window_hours: u64,
    pub max_concurrent_tokens: u32,
    pub max_total_tokens: u32,
}

/// Score every combo. Returns archive rows in combo-index order plus the
/// unguarded fire counts used for the selective-claim enter%.
pub fn score_combos(
    tokens: &[CorpusToken],
    combos: &[GeneratedCombo],
    cfg: &ScoreConfig<'_>,
    observer: &dyn SweepObserver,
) -> anyhow::Result<Vec<ArchiveRow>> {
    if combos.is_empty() {
        return Ok(Vec::new());
    }
    let columns = columns_for(combos, cfg.flow_fp, cfg.flow.is_some());
    let grid = grid_for(combos, &cfg.as_of, tokens);
    let tail = frozen_tail_horizon(cfg.as_of, tokens);
    let bounds: Vec<BoundCombo> = combos
        .iter()
        .map(|c| BoundCombo::new(&columns, compile_params(&c.params, cfg)))
        .collect();

    // Group combos that share an entry filling so the candidate walk runs once.
    let classes = entry_classes(combos);
    let n_tokens = tokens.len();
    observer.set_total(n_tokens, combos.len().max(1));
    let done = AtomicUsize::new(0);

    let per_token: Vec<Vec<TokenOutcome>> = tokens
        .par_iter()
        .enumerate()
        .map(|(_ti, token)| {
            if observer.cancelled() {
                return vec![TokenOutcome::no_entry(); combos.len()];
            }
            let series = build_series_with_flow(
                token,
                columns.clone(),
                &grid,
                cfg.as_of,
                cfg.flow,
            );
            let mut outs = vec![TokenOutcome::no_entry(); combos.len()];
            for idxs in &classes {
                if idxs.is_empty() {
                    continue;
                }
                let mut cands = EntryCandidates::default();
                entry_candidates(&series, &bounds[idxs[0]], &mut cands);
                for &ci in idxs {
                    let entry = resolve_entry_from(
                        &token.trades,
                        &series,
                        &bounds[ci],
                        &mut cands,
                        &cfg.pricing,
                    );
                    outs[ci] = resolve_exit(
                        &token.trades,
                        &series,
                        &bounds[ci],
                        &entry,
                        &cfg.pricing,
                        tail,
                    );
                }
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n == n_tokens || n.is_multiple_of(8) {
                observer.token_done(combos.len());
            }
            outs
        })
        .collect();

    if observer.cancelled() {
        anyhow::bail!("cancelled");
    }

    let range = corpus_range(tokens.iter().map(|t| t.created_at));
    Ok(merge_archive(tokens, combos, &per_token, cfg, range))
}

fn compile_params(params: &hunter_engine::rule_params::RuleParams, cfg: &ScoreConfig<'_>) -> CompiledRule {
    let loaded = LoadedRule {
        id: RuleId(Uuid::nil()),
        fingerprint_id: cfg.flow_fp,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: sol_to_lamports(cfg.pricing.buy_amount_sol).max(0) as u64,
        max_concurrent_tokens: 0,
        max_total_tokens: 0,
        params: params.clone(),
        entry_enabled: true,
    };
    CompiledRule::compile(&loaded)
}

fn entry_classes(combos: &[GeneratedCombo]) -> Vec<Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, c) in combos.iter().enumerate() {
        let key = entry_key(&c.entry.clauses);
        map.entry(key).or_default().push(i);
    }
    map.into_values().collect()
}

fn entry_key(clauses: &[Clause]) -> String {
    let mut parts: Vec<String> = clauses
        .iter()
        .map(|c| {
            format!(
                "{:?}:{:?}:{:?}:{:?}:{}",
                c.group,
                c.metric,
                c.window,
                c.op,
                c.threshold.to_bits()
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

fn columns_for(combos: &[GeneratedCombo], flow_fp: FingerprintId, with_flow: bool) -> Vec<SeriesColumn> {
    let fp = if flow_fp.0.is_nil() { SWEEP_FLOW_FP } else { flow_fp };
    let mut cols = Vec::new();
    let mut push = |c: &Clause| {
        let g = group_spec(c.group);
        if g.scope == MetricScope::Position {
            return;
        }
        if !with_flow && hunter_engine::metrics::is_fingerprint_scoped(c.metric) {
            return;
        }
        let col = if hunter_engine::metrics::is_fingerprint_scoped(c.metric) {
            SeriesColumn::Fingerprint(c.metric, c.window, fp)
        } else {
            match (g.kind, c.window) {
                (MetricKind::Dynamic, Some(w)) => SeriesColumn::window(c.metric, w),
                _ => SeriesColumn::Static(c.metric),
            }
        };
        if !cols.contains(&col) {
            cols.push(col);
        }
    };
    for combo in combos {
        for c in combo.entry.clauses.iter().chain(combo.exit.clauses.iter()) {
            push(c);
        }
    }
    if cols.is_empty() {
        cols.push(SeriesColumn::Static(MetricId::Time));
    }
    cols
}

fn grid_for(combos: &[GeneratedCombo], as_of: &DateTime<Utc>, tokens: &[CorpusToken]) -> SparseGrid {
    let mut max_window = 0.0f64;
    let mut time_h = 0.0f64;
    let mut stall_h = 0.0f64;
    for c in combos {
        for cl in c.entry.clauses.iter().chain(c.exit.clauses.iter()) {
            if let Some(w) = cl.window {
                // A wall clock: a slot span converts at the nominal slot time, and a
                // print span contributes nothing (no tick can move its cursor).
                // Sizes a horizon only, never a reading.
                max_window = max_window.max(match w.unit {
                    hunter_engine::metrics::WindowUnit::Sec => w.size + w.lag,
                    hunter_engine::metrics::WindowUnit::Slot => {
                        (w.size + w.lag) * hunter_engine::metrics::NOMINAL_SLOT_SECS
                    }
                    hunter_engine::metrics::WindowUnit::Print => 0.0,
                });
            }
            if cl.metric == MetricId::Time {
                time_h = time_h.max(cl.threshold);
            }
            if cl.metric == MetricId::Stall {
                stall_h = stall_h.max(cl.threshold);
            }
        }
    }
    if time_h <= 0.0 {
        time_h = tokens
            .iter()
            .filter_map(|t| {
                let last = t.trades.last()?.block_time;
                Some((last - t.created_at).num_seconds() as f64)
            })
            .fold(120.0, f64::max);
    }
    let _ = as_of;
    SparseGrid {
        max_window_secs: max_window,
        time_horizon_secs: time_h,
        stall_horizon_secs: stall_h,
    }
}

struct Fired {
    token_idx: usize,
    identity: Option<u64>,
    entry_time: DateTime<Utc>,
    exit_time: Option<DateTime<Utc>>,
    outcome: TokenOutcome,
}

fn merge_archive(
    tokens: &[CorpusToken],
    combos: &[GeneratedCombo],
    per_token: &[Vec<TokenOutcome>],
    cfg: &ScoreConfig<'_>,
    range: (DateTime<Utc>, DateTime<Utc>),
) -> Vec<ArchiveRow> {
    let n_combos = combos.len();
    let mut rows = Vec::with_capacity(n_combos);
    for ci in 0..n_combos {
        let mut unguarded = ComboAgg::default();
        let mut fired: Vec<Fired> = Vec::new();
        for (ti, token_outs) in per_token.iter().enumerate() {
            let o = &token_outs[ci];
            unguarded.record(o);
            if o.fired {
                if let Some(at) = o.entry_time {
                    fired.push(Fired {
                        token_idx: ti,
                        identity: tokens[ti].identity,
                        entry_time: at,
                        exit_time: o.exit_time,
                        outcome: *o,
                    });
                }
            }
        }
        let unguarded_m = unguarded.finalize(ci as u32);
        fired.sort_by(|a, b| {
            a.entry_time
                .cmp(&b.entry_time)
                .then_with(|| tokens[a.token_idx].mint.cmp(&tokens[b.token_idx].mint))
        });
        let kept = apply_guards(&fired, tokens, cfg);
        let mut agg = ComboAgg::default();
        let mut block_pnl_sol = [0.0f64; N_BLOCKS];
        for f in &kept {
            agg.record(&f.outcome);
            // Realized only — an open mark must not buy a block its trade never closed.
            if f.outcome.exit != trading_core::strategies::kernel::ExitCode::Open {
                block_pnl_sol[block_of(f.entry_time, range)] += f.outcome.pnl_sol as f64;
            }
        }
        let m = agg.finalize(ci as u32);
        rows.push(ArchiveRow {
            combo_idx: ci,
            n_fired: m.n_fired,
            n_closed: m.n_closed,
            n_tokens: kept.len() as u64,
            n_fired_unguarded: unguarded_m.n_fired,
            total_pnl_sol: m.total_pnl_sol,
            block_pnl_sol,
            profit_factor: m.profit_factor,
            win_rate: m.win_rate,
        });
    }
    rows
}

fn apply_guards<'a>(fired: &'a [Fired], tokens: &[CorpusToken], cfg: &ScoreConfig<'_>) -> Vec<&'a Fired> {
    let window = Duration::hours(cfg.duplicate_identity_window_hours as i64);
    let concurrent_cap = cfg.max_concurrent_tokens; // 0 = unlimited
    let total_cap = cfg.max_total_tokens;
    let mut last_id: HashMap<u64, (DateTime<Utc>, usize)> = HashMap::new();
    let mut open: Vec<(DateTime<Utc>, Option<DateTime<Utc>>)> = Vec::new();
    let mut kept = Vec::new();
    let mut total = 0u32;

    for f in fired {
        if cfg.skip_duplicate_identity {
            if let Some(id) = f.identity {
                if let Some((at, tok)) = last_id.get(&id) {
                    if *tok != f.token_idx && f.entry_time >= *at && f.entry_time - *at < window {
                        continue;
                    }
                }
            }
        }
        if concurrent_cap != 0 {
            open.retain(|(en, ex)| {
                let end = ex.unwrap_or(cfg.as_of);
                *en <= f.entry_time && f.entry_time < end
            });
            if open.len() as u32 >= concurrent_cap {
                continue;
            }
        }
        if total_cap != 0 && total >= total_cap {
            continue;
        }
        if cfg.skip_duplicate_identity {
            if let Some(id) = f.identity {
                last_id.insert(id, (f.entry_time, f.token_idx));
            }
        }
        if concurrent_cap != 0 {
            open.push((f.entry_time, f.exit_time));
        }
        total += 1;
        kept.push(f);
        let _ = tokens;
    }
    kept
}

/// Convert corpus tokens to replay tokens (shared event-queue report).
pub fn to_replay_tokens(tokens: &[CorpusToken]) -> Vec<crate::strategies::replay::ReplayToken> {
    tokens
        .iter()
        .map(|t| crate::strategies::replay::ReplayToken {
            mint: t.mint.clone(),
            symbol: t.symbol.clone(),
            created_at: t.created_at,
            tf: t.fp.clone(),
            trades: t.trades.clone(),
            creator_wallet_hash: None,
            identity: t.identity,
        })
        .collect()
}

pub fn loaded_from_params(
    params: hunter_engine::rule_params::RuleParams,
    fp_id: FingerprintId,
    buy_sol: f64,
    max_concurrent: u32,
    max_total: u32,
) -> LoadedRule {
    LoadedRule {
        id: RuleId(Uuid::new_v4()),
        fingerprint_id: fp_id,
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: sol_to_lamports(buy_sol).max(0) as u64,
        max_concurrent_tokens: max_concurrent,
        max_total_tokens: max_total,
        params,
        entry_enabled: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::strategy::TokenOutcome;
    use chrono::TimeZone;
    use trading_core::strategies::kernel::ExitCode;

    fn fired(ti: usize, id: Option<u64>, secs: i64) -> Fired {
        Fired {
            token_idx: ti,
            identity: id,
            entry_time: Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap(),
            exit_time: Some(Utc.timestamp_opt(1_700_000_000 + secs + 10, 0).unwrap()),
            outcome: TokenOutcome {
                fired: true,
                holding_secs: 10,
                pnl_percent: 1.0,
                pnl_sol: 0.1,
                exit: ExitCode::TakeProfit,
                exit_metric: None,
                exit_operator: None,
                exit_metric_value: None,
                exit_metric_window: None,
                exit_metric_slot: None,
                entry_time: None,
                entry_price: None,
                entry_slot: None,
                exit_time: None,
                exit_price: None,
                exit_slot: None,
            },
        }
    }

    #[test]
    fn blocks_split_the_range_into_quartiles() {
        let lo = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let hi = Utc.timestamp_opt(1_700_000_400, 0).unwrap();
        assert_eq!(block_of(lo, (lo, hi)), 0);
        assert_eq!(block_of(Utc.timestamp_opt(1_700_000_150, 0).unwrap(), (lo, hi)), 1);
        assert_eq!(block_of(hi, (lo, hi)), N_BLOCKS - 1);
        // Degenerate range: everything lands in block 0.
        assert_eq!(block_of(hi, (lo, lo)), 0);
    }

    #[test]
    fn robust_sol_trims_the_best_block() {
        let mut row = ArchiveRow {
            combo_idx: 0,
            n_fired: 10,
            n_closed: 10,
            n_tokens: 10,
            n_fired_unguarded: 10,
            total_pnl_sol: 4.0,
            block_pnl_sol: [1.0, 1.0, 1.0, 1.0],
            profit_factor: Some(1.5),
            win_rate: 0.5,
        };
        assert!((row.robust_sol() - 3.0).abs() < 1e-9, "steady edge keeps 3 of 4");
        // Same total, all in one burst — trimmed to zero.
        row.block_pnl_sol = [0.0, 4.0, 0.0, 0.0];
        assert!((row.robust_sol() - 0.0).abs() < 1e-9, "one-burst edge trims away");
        // A negative best block is not subtracted (nothing to trim).
        row.block_pnl_sol = [-1.0, -1.0, -1.0, -1.0];
        row.total_pnl_sol = -4.0;
        assert!((row.robust_sol() + 4.0).abs() < 1e-9);
    }

    #[test]
    fn copycat_drops_later_same_identity() {
        let a = fired(0, Some(42), 0);
        let b = fired(1, Some(42), 5);
        let cfg = ScoreConfig {
            pricing: crate::discovery::fixtures::pricing(),
            as_of: Utc::now(),
            flow: None,
            flow_fp: FingerprintId(Uuid::nil()),
            skip_duplicate_identity: true,
            duplicate_identity_window_hours: 24,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
        };
        let tokens: Vec<CorpusToken> = vec![];
        let fired = [a, b];
        let kept = apply_guards(&fired, &tokens, &cfg);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].token_idx, 0);
    }

    #[test]
    fn copycat_off_keeps_both() {
        let a = fired(0, Some(42), 0);
        let b = fired(1, Some(42), 5);
        let cfg = ScoreConfig {
            pricing: crate::discovery::fixtures::pricing(),
            as_of: Utc::now(),
            flow: None,
            flow_fp: FingerprintId(Uuid::nil()),
            skip_duplicate_identity: false,
            duplicate_identity_window_hours: 24,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
        };
        let fired = [a, b];
        let kept = apply_guards(&fired, &[], &cfg);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn concurrent_cap_blocks() {
        let a = fired(0, Some(1), 0);
        let b = fired(1, Some(2), 1);
        let cfg = ScoreConfig {
            pricing: crate::discovery::fixtures::pricing(),
            as_of: Utc::now(),
            flow: None,
            flow_fp: FingerprintId(Uuid::nil()),
            skip_duplicate_identity: false,
            duplicate_identity_window_hours: 24,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
        };
        let fired = [a, b];
        let kept = apply_guards(&fired, &[], &cfg);
        assert_eq!(kept.len(), 1);
    }
}
