//! Report columns are `run_replay` ([rule-search.md] Scorer). Champion is the
//! replay winner when the fast archive and replay disagree — ranked by
//! spread-discounted authority SOL among paying, sign-agreeing replays, then
//! gated by the 1 s latency ladder.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use hunter_engine::event::{ExitReason, LoadedRule};
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::rule_params::RuleParams;
use trading_core::strategies::kernel::{CostModel, ExitCode};
use trading_core::strategies::paper_fill::FillModel;

use crate::strategies::replay::{run_replay, PositionOutcome, ReplayConfig, ReplayToken};
use crate::sweep::aggregate::ComboAgg;
use crate::sweep::strategy::TokenOutcome;

use super::generator::{
    assemble, clause_label, is_empty_entry, same_exit_bag, EntryFilling, ExitBag, GeneratedCombo,
};
use super::scorer::{block_of, loaded_from_params, ArchiveRow, N_BLOCKS};

/// n floor counts distinct entered TOKENS, not closed trades — copycat-merged,
/// burst-clustered trades are correlated, so trades overstate evidence.
pub const MIN_TOKENS: u64 = 8;
pub const TOP_ARCHIVE: usize = 12;
/// Spread discount in the champion rank: authority − 0.25 × |first − authority|.
/// A violent fill window pays rent instead of winning authority ties by dust.
const SPREAD_DISCOUNT: f64 = 0.25;
/// A paying rule's mean trade must clear the round-trip cost by this multiple.
const EXPECTANCY_FLOOR_MULT: f64 = 2.0;
/// How many top candidates race the 1 s ladder gate.
const LADDER_POOL: usize = 4;
/// Champion decay-curve rungs (ms); the last is the pass/fail gate.
pub const LADDER_DELAYS_MS: [i64; 3] = [250, 500, 1000];
const LADDER_GATE_MS: i64 = 1000;
/// Sibling z-score: champion must clear its same-exit-bag archive siblings'
/// scatter by this many standard deviations, or the edge is selection noise.
const MIN_SIBLINGS: usize = 8;
const MIN_SIBLING_Z: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Refuse,
    Ungated,
    Candidate,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScoredRule {
    pub params: serde_json::Value,
    pub n_fired: u64,
    pub n_closed: u64,
    pub n_tokens_entered: u64,
    pub enter_pct: f64,
    pub enter_pct_unguarded: Option<f64>,
    pub total_pnl_sol: f64,
    pub total_pnl_sol_optimistic: Option<f64>,
    pub profit_factor: Option<f64>,
    pub win_rate: f64,
    /// Mean realized SOL per closed trade (`None` when nothing closed).
    #[serde(default)]
    pub expectancy_sol: Option<f64>,
    /// Realized SOL per time quartile of the search range — a habit that died
    /// mid-range shows as a front-loaded row.
    #[serde(default)]
    pub block_pnl_sol: Vec<f64>,
}

/// One latency-ladder rung: the champion replayed with both legs' fill windows
/// opened `delay_ms` after each decision.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LadderRung {
    pub delay_ms: i64,
    pub total_pnl_sol: f64,
    pub n_closed: u64,
}

/// Champion minus one clause, replayed under authority fill.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AblationRow {
    /// `entry` or `exit`.
    pub side: String,
    /// The removed clause's label.
    pub removed: String,
    pub total_pnl_sol: f64,
    pub n_tokens_entered: u64,
    pub enter_pct: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub verdict: Verdict,
    pub n_matched: u64,
    pub n_combos: u64,
    pub champion: Option<ScoredRule>,
    pub empty_entry: Option<ScoredRule>,
    pub incumbent: Option<ScoredRule>,
    pub archive: Vec<ScoredRule>,
    pub archive_replay_disagree: bool,
    pub diagnostics: Vec<String>,
    /// Champion authority SOL at 0 / 250 / 500 / 1000 ms fill delay.
    #[serde(default)]
    pub champion_ladder: Vec<LadderRung>,
    /// Every ladder-raced candidate flips sign at 1 s — the edge needs a fill
    /// the box cannot get.
    #[serde(default)]
    pub latency_fragile: bool,
    /// Champion's fast-archive SOL vs same-exit-bag siblings, in standard
    /// deviations. `None` when the sibling set is too thin.
    #[serde(default)]
    pub sibling_z: Option<f64>,
    #[serde(default)]
    pub champion_ablation: Vec<AblationRow>,
    /// Champion realized SOL over gross attainable (entry → post-entry ATH).
    #[serde(default)]
    pub exit_efficiency: Option<f64>,
}

pub struct ReplayOpts {
    pub as_of: DateTime<Utc>,
    pub fill_authority: FillModel,
    pub fill_optimistic: FillModel,
    pub cost: CostModel,
    pub skip_duplicate_identity: bool,
    pub duplicate_identity_window_hours: u64,
    pub buy_sol: f64,
    /// `[min, max]` token `created_at` of the corpus — the quartile axis.
    pub range: (DateTime<Utc>, DateTime<Utc>),
}

/// One combo's replay bundle: authority row, first-in-window SOL, and the
/// authority outcomes (kept for the champion's exit-efficiency pass).
struct Replayed {
    row: ScoredRule,
    opti_sol: f64,
    outcomes: Vec<PositionOutcome>,
}

pub fn build_report(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    combos: &[GeneratedCombo],
    archive: &[ArchiveRow],
    n_matched: u64,
    incumbent_loaded: Option<LoadedRule>,
    opts: &ReplayOpts,
) -> Report {
    let mut diagnostics = Vec::new();
    let ranked = rank_archive(archive);
    let fast_best = ranked.first().copied();
    let floor = expectancy_floor_sol(opts.buy_sol, &opts.cost);

    // Report columns are `run_replay`. Replay the whole board (not a shorter
    // prefix) so a lower-ranked fast row cannot show a series-walk SOL the
    // champion never raced.
    let mut replayed: HashMap<usize, Replayed> = HashMap::new();
    for &i in ranked.iter().take(TOP_ARCHIVE) {
        replayed.insert(i, replay_combo(tokens, fp, &combos[i].params, opts));
    }

    let incumbent_row = incumbent_loaded.as_ref().map(|loaded| {
        let r = replay_pair(tokens, fp, loaded, opts);
        let mut row = r.row;
        row.total_pnl_sol_optimistic = Some(r.opti_sol);
        row
    });

    // Fill-moment rules can crowd the fast-archive head, then lose on replay —
    // and a sign-disagreeing (violent-window) winner is not a paying rule.
    // One extra slice gives a quieter peak-contrast row a chance to be champion.
    if !slice_pays(&replayed, floor) && ranked.len() > TOP_ARCHIVE {
        diagnostics.push(
            "No paying, sign-agreeing rule in the top archive slice; scoring the next slice."
                .into(),
        );
        for &i in ranked.iter().skip(TOP_ARCHIVE).take(TOP_ARCHIVE) {
            replayed.insert(i, replay_combo(tokens, fp, &combos[i].params, opts));
        }
    }

    // Paying, sign-agreeing candidates by spread-discounted authority; then the
    // 1 s ladder gate picks the first latency-robust one as champion.
    let paying = ranked_paying(&replayed, floor);
    let mut latency_fragile = false;
    let mut ladder_1s: HashMap<usize, LadderRung> = HashMap::new();
    let replay_best = if paying.is_empty() {
        ranked_all(&replayed).first().copied()
    } else {
        let mut chosen = paying[0];
        let mut passed = false;
        for &i in paying.iter().take(LADDER_POOL) {
            let (row, _) = replay_one(tokens, fp, &combos[i].params, opts, LADDER_GATE_MS);
            let rung = LadderRung {
                delay_ms: LADDER_GATE_MS,
                total_pnl_sol: row.total_pnl_sol,
                n_closed: row.n_closed,
            };
            let pass = rung.total_pnl_sol > 0.0;
            ladder_1s.insert(i, rung);
            if pass {
                chosen = i;
                passed = true;
                break;
            }
        }
        if !passed {
            latency_fragile = true;
            diagnostics.push(
                "Every ladder-raced candidate flips sign at 1 s fill delay — the edge is latency-fragile."
                    .into(),
            );
        } else if chosen != paying[0] {
            diagnostics.push(
                "Top-ranked rule flips sign at 1 s fill delay; champion is the best ladder-robust rule."
                    .into(),
            );
        }
        Some(chosen)
    };

    let archive_replay_disagree = matches!((fast_best, replay_best), (Some(a), Some(b)) if a != b);
    if archive_replay_disagree {
        diagnostics.push(
            "Fast archive winner and replay winner disagree — champion is the replay one.".into(),
        );
    }

    let champ_idx = replay_best.or(fast_best);
    // Empty-entry shares the champion's exit bag, not the fast-archive winner's.
    let empty_idx = champ_idx.and_then(|i| {
        combos
            .iter()
            .position(|c| is_empty_entry(&c.entry) && same_exit_bag(&c.exit, &combos[i].exit))
    });
    if let Some(i) = empty_idx {
        replayed
            .entry(i)
            .or_insert_with(|| replay_combo(tokens, fp, &combos[i].params, opts));
    }

    // Champion decay curve: 0 (the authority row) + 250 / 500 / 1000 ms.
    let mut champion_ladder = Vec::new();
    if let Some(i) = champ_idx {
        if !paying.is_empty() && replayed.contains_key(&i) {
            let base = &replayed[&i].row;
            champion_ladder.push(LadderRung {
                delay_ms: 0,
                total_pnl_sol: base.total_pnl_sol,
                n_closed: base.n_closed,
            });
            for delay in LADDER_DELAYS_MS {
                if delay == LADDER_GATE_MS {
                    if let Some(r) = ladder_1s.get(&i) {
                        champion_ladder.push(r.clone());
                        continue;
                    }
                }
                let (row, _) = replay_one(tokens, fp, &combos[i].params, opts, delay);
                champion_ladder.push(LadderRung {
                    delay_ms: delay,
                    total_pnl_sol: row.total_pnl_sol,
                    n_closed: row.n_closed,
                });
            }
        }
    }

    if let Some(i) = champ_idx {
        let phases: Vec<&str> = combos[i]
            .entry
            .clauses
            .iter()
            .chain(combos[i].exit.clauses.iter())
            .map(|c| c.phase.label())
            .collect();
        if !phases.is_empty() {
            diagnostics.push(format!("Champion cut phases: {}.", phases.join(", ")));
        }
        if let Some(r) = replayed.get(&i) {
            if !sign_agrees(r.row.total_pnl_sol, r.opti_sol) {
                diagnostics.push(
                    "Champion fill window is violent: authority and first-in-window disagree in sign.".into(),
                );
            }
        }
    }
    let mut champion = champ_idx.and_then(|i| {
        replayed.get(&i).map(|r| {
            let mut row = r.row.clone();
            row.total_pnl_sol_optimistic = Some(r.opti_sol);
            if let Some(a) = archive.get(i) {
                row.enter_pct_unguarded = Some(a.enter_pct(n_matched));
            }
            row
        })
    });

    let empty_entry = empty_idx.and_then(|i| {
        replayed.get(&i).map(|r| {
            let mut row = r.row.clone();
            row.total_pnl_sol_optimistic = Some(r.opti_sol);
            row
        })
    });

    // Champion's fast-archive edge vs its same-exit-bag siblings' scatter —
    // a max that sits inside the noise was selected, not found.
    let sibling_z = champ_idx.and_then(|i| sibling_z_of(i, combos, archive));

    let mut verdict = verdict_of(
        champion.as_ref(),
        empty_entry.as_ref(),
        &replayed,
        combos,
        archive,
        floor,
    );
    if verdict == Verdict::Candidate {
        if let Some(z) = sibling_z {
            if z < MIN_SIBLING_Z {
                verdict = Verdict::Ungated;
                diagnostics.push(format!(
                    "Champion beats empty-entry by less than its archive siblings' scatter (z = {z:.2}) — selection noise, not a selector."
                ));
            }
        }
    }

    if matches!(verdict, Verdict::Refuse) {
        diagnostics.push(
            "Refuse is a finished run — paper the next launch burst, or pick a new range.".into(),
        );
    }
    if matches!(verdict, Verdict::Ungated) {
        diagnostics.push(
            "Juice is ungated: empty-entry (buy everything) is not beaten by a selector.".into(),
        );
    }

    // Per-clause ablation: the champion minus each clause, authority replay.
    let champion_ablation = champ_idx
        .map(|i| ablation_rows(tokens, fp, &combos[i], opts))
        .unwrap_or_default();

    // Exit efficiency: realized over gross attainable (post-entry ATH).
    let exit_efficiency = champ_idx
        .and_then(|i| replayed.get(&i))
        .and_then(|r| exit_efficiency_of(r, tokens, opts.buy_sol));

    let mut archive_out: Vec<ScoredRule> = ranked
        .iter()
        .filter(|i| replayed.contains_key(i))
        .filter_map(|&i| {
            replayed.get(&i).map(|r| {
                let mut row = r.row.clone();
                row.total_pnl_sol_optimistic = Some(r.opti_sol);
                row
            })
        })
        .collect();
    archive_out.sort_by(|a, b| {
        b.total_pnl_sol
            .partial_cmp(&a.total_pnl_sol)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    archive_out.truncate(TOP_ARCHIVE);

    if champion.is_none() {
        champion = archive_out.first().cloned();
    }

    Report {
        verdict,
        n_matched,
        n_combos: combos.len() as u64,
        champion,
        empty_entry,
        incumbent: incumbent_row,
        archive: archive_out,
        archive_replay_disagree,
        diagnostics,
        champion_ladder,
        latency_fragile,
        sibling_z,
        champion_ablation,
        exit_efficiency,
    }
}

/// Fast-archive rank: trimmed (worst-3-of-4-blocks) SOL first, so a one-burst
/// edge cannot buy the whole top slice; total, then n break ties.
fn rank_archive(archive: &[ArchiveRow]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..archive.len()).collect();
    idx.sort_by(|&a, &b| {
        archive[b]
            .robust_sol()
            .partial_cmp(&archive[a].robust_sol())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                archive[b]
                    .total_pnl_sol
                    .partial_cmp(&archive[a].total_pnl_sol)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| archive[b].n_closed.cmp(&archive[a].n_closed))
    });
    idx
}

/// 2× the round-trip cost of one trade at this buy size — the least a mean
/// trade must clear for total SOL to mean edge rather than trade count.
fn expectancy_floor_sol(buy_sol: f64, cost: &CostModel) -> f64 {
    let per_leg = buy_sol * (cost.fee_bps_per_leg + cost.slippage_bps) / 10_000.0
        + cost.fixed_cost_sol_per_leg;
    EXPECTANCY_FLOOR_MULT * 2.0 * per_leg
}

fn sign_agrees(auth: f64, opti: f64) -> bool {
    auth == 0.0 || opti == 0.0 || (auth > 0.0) == (opti > 0.0)
}

fn discounted(r: &Replayed) -> f64 {
    r.row.total_pnl_sol - SPREAD_DISCOUNT * fill_spread(&r.row, r.opti_sol)
}

/// Paying, sign-agreeing replays ranked by spread-discounted authority.
fn ranked_paying(replayed: &HashMap<usize, Replayed>, floor: f64) -> Vec<usize> {
    let mut idx: Vec<usize> = replayed
        .iter()
        .filter(|(_, r)| pays(&r.row, floor) && sign_agrees(r.row.total_pnl_sol, r.opti_sol))
        .map(|(&i, _)| i)
        .collect();
    idx.sort_by(|&a, &b| {
        discounted(&replayed[&b])
            .partial_cmp(&discounted(&replayed[&a]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| replayed[&b].row.n_tokens_entered.cmp(&replayed[&a].row.n_tokens_entered))
    });
    idx
}

/// Every replay ranked by spread-discounted authority (fallback pool).
fn ranked_all(replayed: &HashMap<usize, Replayed>) -> Vec<usize> {
    let mut idx: Vec<usize> = replayed.keys().copied().collect();
    idx.sort_by(|&a, &b| {
        discounted(&replayed[&b])
            .partial_cmp(&discounted(&replayed[&a]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| replayed[&b].row.n_closed.cmp(&replayed[&a].row.n_closed))
    });
    idx
}

fn fill_spread(row: &ScoredRule, opti: f64) -> f64 {
    (opti - row.total_pnl_sol).abs()
}

fn slice_pays(replayed: &HashMap<usize, Replayed>, floor: f64) -> bool {
    replayed
        .values()
        .any(|r| pays(&r.row, floor) && sign_agrees(r.row.total_pnl_sol, r.opti_sol))
}

fn sibling_z_of(champ: usize, combos: &[GeneratedCombo], archive: &[ArchiveRow]) -> Option<f64> {
    let vals: Vec<f64> = combos
        .iter()
        .enumerate()
        .filter(|(j, c)| {
            *j != champ
                && !is_empty_entry(&c.entry)
                && same_exit_bag(&c.exit, &combos[champ].exit)
        })
        .filter_map(|(j, _)| archive.get(j).map(|a| a.total_pnl_sol))
        .collect();
    if vals.len() < MIN_SIBLINGS {
        return None;
    }
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64;
    let sd = var.sqrt();
    if sd <= 1e-12 {
        return None;
    }
    Some((archive.get(champ)?.total_pnl_sol - mean) / sd)
}

fn ablation_rows(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    combo: &GeneratedCombo,
    opts: &ReplayOpts,
) -> Vec<AblationRow> {
    let n_clauses = combo.entry.clauses.len() + combo.exit.clauses.len();
    if n_clauses < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut run = |entry: &EntryFilling, exit: &ExitBag, side: &str, removed: String| {
        let params = assemble(entry, exit);
        let (row, _) = replay_one(tokens, fp, &params, opts, 0);
        out.push(AblationRow {
            side: side.to_string(),
            removed,
            total_pnl_sol: row.total_pnl_sol,
            n_tokens_entered: row.n_tokens_entered,
            enter_pct: row.enter_pct,
        });
    };
    for (k, c) in combo.entry.clauses.iter().enumerate() {
        let mut entry = combo.entry.clone();
        entry.clauses.remove(k);
        run(&entry, &combo.exit, "entry", clause_label(c));
    }
    for (k, c) in combo.exit.clauses.iter().enumerate() {
        let mut exit = combo.exit.clone();
        exit.clauses.remove(k);
        run(&combo.entry, &exit, "exit", clause_label(c));
    }
    out
}

fn exit_efficiency_of(r: &Replayed, tokens: &[ReplayToken], buy_sol: f64) -> Option<f64> {
    let by_mint: HashMap<&str, &ReplayToken> =
        tokens.iter().map(|t| (t.mint.as_str(), t)).collect();
    let mut attainable = 0.0;
    for o in &r.outcomes {
        let Some(tok) = by_mint.get(o.mint.as_str()) else {
            continue;
        };
        let peak = tok
            .trades
            .iter()
            .filter(|t| t.block_time >= o.entry_time)
            .map(|t| t.price_per_token)
            .filter(|p| p.is_finite())
            .fold(f64::NEG_INFINITY, f64::max);
        if peak.is_finite() && o.entry_price > 0.0 {
            attainable += (buy_sol * (peak / o.entry_price - 1.0)).max(0.0);
        }
    }
    (attainable > 1e-9).then(|| r.row.total_pnl_sol / attainable)
}

fn replay_combo(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    params: &RuleParams,
    opts: &ReplayOpts,
) -> Replayed {
    let loaded = loaded_from_params(params.clone(), fp.id, opts.buy_sol, 0, 0);
    replay_pair(tokens, fp, &loaded, opts)
}

fn replay_pair(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    loaded: &LoadedRule,
    opts: &ReplayOpts,
) -> Replayed {
    let (auth, outcomes) = replay_loaded(tokens, fp, loaded, opts, opts.fill_authority, 0);
    let (opti, _) = replay_loaded(tokens, fp, loaded, opts, opts.fill_optimistic, 0);
    Replayed {
        row: auth,
        opti_sol: opti.total_pnl_sol,
        outcomes,
    }
}

/// Authority-only replay of raw params (ladder rungs + ablation rows).
fn replay_one(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    params: &RuleParams,
    opts: &ReplayOpts,
    fill_delay_ms: i64,
) -> (ScoredRule, Vec<PositionOutcome>) {
    let loaded = loaded_from_params(params.clone(), fp.id, opts.buy_sol, 0, 0);
    replay_loaded(tokens, fp, &loaded, opts, opts.fill_authority, fill_delay_ms)
}

fn replay_loaded(
    tokens: &[ReplayToken],
    fp: &EngineFingerprint,
    loaded: &LoadedRule,
    opts: &ReplayOpts,
    fill: FillModel,
    fill_delay_ms: i64,
) -> (ScoredRule, Vec<PositionOutcome>) {
    let outcomes = run_replay(
        std::slice::from_ref(loaded),
        std::slice::from_ref(fp),
        tokens.to_vec(),
        ReplayConfig {
            as_of: opts.as_of,
            fill_model: fill,
            skip_duplicate_identity: opts.skip_duplicate_identity,
            duplicate_identity_window_hours: opts.duplicate_identity_window_hours,
            fill_delay_ms,
            // The lake corpus carries no creator wallet, so `m_snapshot.prior_launches`
            // cannot be primed here and reads `NaN` (see `LAKE_BLIND_METRICS`).
            creator_launches: Default::default(),
        },
    );
    let row = summarize(
        &outcomes,
        tokens.len() as u64,
        loaded.params.to_value(),
        opts.buy_sol,
        &opts.cost,
        opts.range,
    );
    (row, outcomes)
}

fn summarize(
    outcomes: &[PositionOutcome],
    n_matched: u64,
    params: serde_json::Value,
    buy_sol: f64,
    cost: &CostModel,
    range: (DateTime<Utc>, DateTime<Utc>),
) -> ScoredRule {
    let mut agg = ComboAgg::default();
    let mut mints = HashSet::new();
    let mut block_pnl_sol = [0.0f64; N_BLOCKS];
    for o in outcomes {
        mints.insert(o.mint.as_str());
        let t = replay_to_token(o, buy_sol, cost);
        // Realized only — an open mark must not buy a block its trade never closed.
        if t.exit != ExitCode::Open {
            block_pnl_sol[block_of(o.entry_time, range)] += t.pnl_sol as f64;
        }
        agg.record(&t);
    }
    let m = agg.finalize(0);
    let enter_pct = if n_matched == 0 {
        0.0
    } else {
        mints.len() as f64 / n_matched as f64
    };
    ScoredRule {
        params,
        n_fired: m.n_fired,
        n_closed: m.n_closed,
        n_tokens_entered: mints.len() as u64,
        enter_pct,
        enter_pct_unguarded: None,
        total_pnl_sol: m.total_pnl_sol,
        total_pnl_sol_optimistic: None,
        profit_factor: m.profit_factor,
        win_rate: m.win_rate,
        expectancy_sol: (m.n_closed > 0).then_some(m.expectancy_sol),
        block_pnl_sol: block_pnl_sol.to_vec(),
    }
}

fn replay_to_token(po: &PositionOutcome, buy_sol: f64, cost: &CostModel) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = po.pnl_with_costs(buy_sol, cost);
    TokenOutcome {
        fired: true,
        holding_secs: po
            .exit_time
            .map(|t| (t - po.entry_time).num_seconds())
            .unwrap_or(0),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: match po.exit_reason {
            None => ExitCode::Open,
            Some(ExitReason::TakeProfit) => ExitCode::TakeProfit,
            Some(ExitReason::StopLoss) => ExitCode::StopLoss,
            Some(ExitReason::Metrics { .. }) => ExitCode::Metrics,
            Some(ExitReason::Dead) => ExitCode::Dead,
            Some(ExitReason::Manual | ExitReason::Migrated) => ExitCode::Open,
        },
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_window: None,
        exit_metric_slot: None,
        entry_time: Some(po.entry_time),
        entry_price: Some(po.entry_price),
        entry_slot: None,
        exit_time: po.exit_time,
        exit_price: po.exit_price,
        exit_slot: None,
    }
}

/// A paying rule: enough distinct tokens (the effective n), positive authority
/// SOL, PF above 1, and a mean trade that clears the expectancy floor.
fn pays(row: &ScoredRule, expectancy_floor: f64) -> bool {
    row.n_tokens_entered >= MIN_TOKENS
        && row.total_pnl_sol > 0.0
        && row.profit_factor.map(|p| p > 1.0).unwrap_or(true)
        && row.expectancy_sol.map(|e| e >= expectancy_floor).unwrap_or(false)
}

fn verdict_of(
    champion: Option<&ScoredRule>,
    empty: Option<&ScoredRule>,
    replayed: &HashMap<usize, Replayed>,
    combos: &[GeneratedCombo],
    archive: &[ArchiveRow],
    expectancy_floor: f64,
) -> Verdict {
    let empty_pays = empty.map(|r| pays(r, expectancy_floor)).unwrap_or(false);
    let mut other_pays = champion.map(|r| pays(r, expectancy_floor)).unwrap_or(false);
    for (i, r) in replayed {
        if is_empty_entry(&combos[*i].entry) {
            continue;
        }
        if pays(&r.row, expectancy_floor) {
            other_pays = true;
            break;
        }
    }
    if !other_pays {
        for (i, a) in archive.iter().enumerate() {
            if is_empty_entry(&combos[i].entry) {
                continue;
            }
            if a.pays(MIN_TOKENS, expectancy_floor) {
                other_pays = true;
                break;
            }
        }
    }

    if !empty_pays && !other_pays {
        return Verdict::Refuse;
    }
    if other_pays {
        if let (Some(ch), Some(em)) = (champion, empty) {
            if pays(ch, expectancy_floor) && ch.total_pnl_sol > em.total_pnl_sol + 1e-9 {
                return Verdict::Candidate;
            }
            if empty_pays && !pays(ch, expectancy_floor) {
                return Verdict::Ungated;
            }
            if pays(ch, expectancy_floor) && ch.total_pnl_sol <= em.total_pnl_sol + 1e-9 {
                return Verdict::Ungated;
            }
        }
        return Verdict::Candidate;
    }
    Verdict::Ungated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pnl: f64, closed: u64, pf: f64) -> ScoredRule {
        ScoredRule {
            params: serde_json::json!({}),
            n_fired: closed,
            n_closed: closed,
            n_tokens_entered: closed,
            enter_pct: 0.2,
            enter_pct_unguarded: None,
            total_pnl_sol: pnl,
            total_pnl_sol_optimistic: None,
            profit_factor: Some(pf),
            win_rate: 0.5,
            expectancy_sol: (closed > 0).then(|| pnl / closed as f64),
            block_pnl_sol: vec![pnl / 4.0; 4],
        }
    }

    fn bundle(pnl: f64, closed: u64, pf: f64, opti: f64) -> Replayed {
        Replayed {
            row: row(pnl, closed, pf),
            opti_sol: opti,
            outcomes: Vec::new(),
        }
    }

    #[test]
    fn all_lose_is_refuse() {
        let ch = row(-1.0, 20, 0.5);
        let em = row(-2.0, 20, 0.4);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[], 0.0),
            Verdict::Refuse
        );
    }

    #[test]
    fn empty_pays_others_lose_is_ungated() {
        let ch = row(-1.0, 20, 0.5);
        let em = row(2.0, 20, 1.5);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[], 0.0),
            Verdict::Ungated
        );
    }

    #[test]
    fn selector_beats_empty_is_candidate() {
        let ch = row(5.0, 20, 1.8);
        let em = row(1.0, 20, 1.2);
        assert_eq!(
            verdict_of(Some(&ch), Some(&em), &HashMap::new(), &[], &[], 0.0),
            Verdict::Candidate
        );
    }

    #[test]
    fn expectancy_floor_gates_pays() {
        // 20 closed, 0.113 total → mean 0.00565/trade. A floor above that ⇒ not paying.
        let r = row(0.113, 20, 1.2);
        assert!(pays(&r, 0.005));
        assert!(!pays(&r, 0.006));
    }

    #[test]
    fn token_floor_gates_pays() {
        let mut r = row(2.0, 20, 1.5);
        r.n_tokens_entered = MIN_TOKENS - 1;
        assert!(!pays(&r, 0.0));
    }

    #[test]
    fn ranked_paying_picks_higher_discounted_authority() {
        let mut replayed = HashMap::new();
        replayed.insert(0, bundle(-0.12, 20, 0.7, 0.545));
        replayed.insert(1, bundle(0.113, 20, 1.2, 0.119));
        assert_eq!(ranked_paying(&replayed, 0.0), vec![1]);
        assert!(slice_pays(&replayed, 0.0));
    }

    #[test]
    fn tighter_spread_wins_at_equal_authority() {
        let mut replayed = HashMap::new();
        replayed.insert(0, bundle(0.113, 20, 1.2, 0.545));
        replayed.insert(1, bundle(0.113, 20, 1.2, 0.119));
        assert_eq!(ranked_paying(&replayed, 0.0), vec![1, 0]);
    }

    #[test]
    fn spread_discount_can_flip_a_dust_authority_lead() {
        let mut replayed = HashMap::new();
        // 0 leads on authority by dust but pays 0.25 × 0.5 spread rent.
        replayed.insert(0, bundle(0.115, 20, 1.2, 0.615));
        replayed.insert(1, bundle(0.113, 20, 1.2, 0.119));
        assert_eq!(ranked_paying(&replayed, 0.0), vec![1, 0]);
    }

    #[test]
    fn sign_disagreement_is_not_a_paying_slice() {
        let mut replayed = HashMap::new();
        // Authority pays, first-in-window loses — a violent fill window.
        replayed.insert(0, bundle(0.5, 20, 1.4, -0.2));
        assert!(ranked_paying(&replayed, 0.0).is_empty());
        assert!(!slice_pays(&replayed, 0.0));
    }

    #[test]
    fn empty_slice_does_not_pay() {
        let mut replayed = HashMap::new();
        replayed.insert(0, bundle(-0.12, 20, 0.7, 0.545));
        assert!(!slice_pays(&replayed, 0.0));
    }

    #[test]
    fn expectancy_floor_scales_with_buy_and_cost() {
        let cost = CostModel {
            fee_bps_per_leg: 125.0,
            slippage_bps: 0.0,
            fixed_cost_sol_per_leg: 0.000225,
            price_impact: true,
        };
        // 2 × round-trip: 2 × 2 × (0.1 × 0.0125 + 0.000225) = 0.0059.
        let f = expectancy_floor_sol(0.1, &cost);
        assert!((f - 0.0059).abs() < 1e-9, "{f}");
    }
}
