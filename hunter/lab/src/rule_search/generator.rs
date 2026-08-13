//! Entry fillings × exit bags → complete `RuleParams` ([rule-search-method.md] §3).
//!
//! No kitchen-sink entry and no greedy add/drop. Empty entry and empty exit are
//! combos. Entry can-fail is 0–2 (selector + extra pooled); trigger stays 0–1.
//! Exit bags are 0–2 different metrics; giveback and clock still compete.

use std::collections::{BTreeMap, HashMap};

use hunter_engine::metrics::evaluator::{Condition, Operator};
use hunter_engine::metrics::{group_spec, MetricGroupId, MetricId, MetricKind};
use hunter_engine::rule_params::{GroupConditions, RuleParams, SideConditions};

use super::cuts::{fill_key, Cut, CutPhase, CutTable, MIN_SPLIT};
use super::roles::{
    entry_compete, entry_role, exit_competes, exit_role, EntryRole, ExitRole, TriggerFamily,
};

/// One authored clause (metric, side implied by which bag it sits in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clause {
    pub group: MetricGroupId,
    pub metric: MetricId,
    pub window: Option<f64>,
    pub op: Operator,
    pub threshold: f64,
    pub phase: CutPhase,
}

impl From<Cut> for Clause {
    fn from(c: Cut) -> Self {
        Self {
            group: c.group,
            metric: c.metric,
            window: c.window,
            op: c.op,
            threshold: c.threshold,
            phase: c.phase,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntryFilling {
    pub clauses: Vec<Clause>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExitBag {
    pub clauses: Vec<Clause>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedCombo {
    pub entry: EntryFilling,
    pub exit: ExitBag,
    pub params: RuleParams,
}

pub fn generate(cuts: &CutTable) -> Vec<GeneratedCombo> {
    let entries = entry_fillings(cuts);
    let exits = exit_bags(&cuts.exit);
    let mut out = Vec::with_capacity(entries.len() * exits.len());
    for e in &entries {
        for x in &exits {
            out.push(GeneratedCombo {
                params: assemble(e, x),
                entry: e.clone(),
                exit: x.clone(),
            });
        }
    }
    out
}

/// Top-3 extra-OR candidates: unused exit cuts as one extra clause on a full rule.
pub fn extra_or_candidates(base: &GeneratedCombo, cuts: &CutTable) -> Vec<GeneratedCombo> {
    let used: Vec<MetricId> = base.exit.clauses.iter().map(|c| c.metric).collect();
    let mut ranked = cuts.exit.clone();
    ranked.sort_by_key(|c| c.phase.menu_rank());
    let mut out = Vec::new();
    for cut in &ranked {
        if used.contains(&cut.metric) {
            continue;
        }
        if base.exit.clauses.iter().any(|c| exit_competes(c.metric, cut.metric)) {
            continue;
        }
        if base.exit.clauses.len() >= 2 {
            continue;
        }
        let mut exit = base.exit.clone();
        exit.clauses.push(Clause::from(*cut));
        out.push(GeneratedCombo {
            params: assemble(&base.entry, &exit),
            entry: base.entry.clone(),
            exit,
        });
        if out.len() >= 16 {
            break;
        }
    }
    out
}

/// Cap on can-fail singles in the product (selector + extra pooled).
const CAN_FAIL_CAP: usize = 10;
const RETUNE_CAP: usize = 12;

fn entry_fillings(cuts: &CutTable) -> Vec<EntryFilling> {
    let mut can_fail: Vec<Clause> = Vec::new();
    let mut triggers: BTreeMap<TriggerFamily, Vec<Clause>> = BTreeMap::new();
    for c in &cuts.entry {
        match entry_role(c.metric) {
            Some(EntryRole::Selector) | Some(EntryRole::Extra) => {
                can_fail.push(Clause::from(*c));
            }
            Some(EntryRole::Trigger(fam)) => {
                triggers.entry(fam).or_default().push(Clause::from(*c));
            }
            _ => {}
        }
    }
    can_fail.sort_by(|a, b| {
        a.phase
            .menu_rank()
            .cmp(&b.phase.menu_rank())
            .then_with(|| format!("{:?}", a.metric).cmp(&format!("{:?}", b.metric)))
    });
    // One cut per metric (best phase), then cap. Contrast outranks FillMoment,
    // so a 1.5× snapshot cannot erase the peak knife for the same metric.
    let mut seen = Vec::new();
    can_fail.retain(|c| {
        if seen.contains(&c.metric) {
            false
        } else {
            seen.push(c.metric);
            true
        }
    });
    can_fail.truncate(CAN_FAIL_CAP);

    let trigger_opts: Vec<Option<Clause>> = {
        let mut v = vec![None];
        for list in triggers.values() {
            let mut ranked = list.clone();
            ranked.sort_by_key(|c| c.phase.menu_rank());
            if let Some(c) = ranked.first() {
                v.push(Some(*c));
            }
        }
        v
    };

    let mut fail_opts: Vec<Vec<Clause>> = vec![vec![]];
    for c in &can_fail {
        fail_opts.push(vec![*c]);
    }
    for i in 0..can_fail.len() {
        for j in (i + 1)..can_fail.len() {
            let pair = [can_fail[i], can_fail[j]];
            if has_entry_compete_clash(&pair) {
                continue;
            }
            if !pair_cooccurs(&pair[0], &pair[1], &cuts.winner_fill) {
                continue;
            }
            fail_opts.push(pair.to_vec());
        }
    }

    let mut out = Vec::new();
    for fails in &fail_opts {
        for t in &trigger_opts {
            let mut clauses = fails.clone();
            if let Some(c) = t {
                clauses.push(*c);
            }
            if has_entry_compete_clash(&clauses) {
                continue;
            }
            out.push(EntryFilling { clauses });
        }
    }
    if out.is_empty() {
        out.push(EntryFilling { clauses: vec![] });
    }
    out
}

fn pair_cooccurs(a: &Clause, b: &Clause, fills: &[HashMap<(MetricId, i64), f64>]) -> bool {
    // Peak contrast pairs are not gated on the 1.5× snapshot — that clock
    // drops knives that only hold later. Fill-moment pairs still must co-occur
    // at fill-moment.
    if a.phase != CutPhase::FillMoment || b.phase != CutPhase::FillMoment {
        return true;
    }
    if fills.len() < MIN_SPLIT {
        return true;
    }
    let n = fills
        .iter()
        .filter(|m| match (m.get(&fill_key(a.metric, a.window)), m.get(&fill_key(b.metric, b.window))) {
            (Some(&va), Some(&vb)) => clause_holds(a, va) && clause_holds(b, vb),
            _ => false,
        })
        .count();
    n >= MIN_SPLIT
}

fn clause_holds(c: &Clause, v: f64) -> bool {
    match c.op {
        Operator::Gt => v > c.threshold,
        Operator::Gte => v >= c.threshold,
        Operator::Lt => v < c.threshold,
        Operator::Lte => v <= c.threshold,
        Operator::Eq => (v - c.threshold).abs() < 1e-9,
        Operator::Ne => (v - c.threshold).abs() >= 1e-9,
    }
}

/// Same-metric neighbors from the table. Does not add a metric.
pub fn retune_candidates(base: &GeneratedCombo, cuts: &CutTable) -> Vec<GeneratedCombo> {
    let mut out = Vec::new();
    for (i, c) in base.entry.clauses.iter().enumerate() {
        for n in neighbors(c, &cuts.entry) {
            let mut entry = base.entry.clone();
            entry.clauses[i] = n;
            if has_entry_compete_clash(&entry.clauses) {
                continue;
            }
            out.push(GeneratedCombo {
                params: assemble(&entry, &base.exit),
                entry,
                exit: base.exit.clone(),
            });
            if out.len() >= RETUNE_CAP {
                return out;
            }
        }
    }
    for (i, c) in base.exit.clauses.iter().enumerate() {
        for n in neighbors(c, &cuts.exit) {
            if base.exit.clauses.iter().enumerate().any(|(j, o)| {
                j != i && (o.metric == n.metric || exit_competes(o.metric, n.metric))
            }) {
                continue;
            }
            let mut exit = base.exit.clone();
            exit.clauses[i] = n;
            out.push(GeneratedCombo {
                params: assemble(&base.entry, &exit),
                entry: base.entry.clone(),
                exit,
            });
            if out.len() >= RETUNE_CAP {
                return out;
            }
        }
    }
    out
}

fn neighbors(c: &Clause, cuts: &[Cut]) -> Vec<Clause> {
    cuts.iter()
        .filter(|n| n.metric == c.metric && n.group == c.group && n.phase == c.phase)
        .filter(|n| {
            n.window != c.window
                || n.op != c.op
                || (n.threshold - c.threshold).abs() > 1e-9
        })
        .copied()
        .map(Clause::from)
        .collect()
}

fn has_entry_compete_clash(clauses: &[Clause]) -> bool {
    for (i, a) in clauses.iter().enumerate() {
        for b in clauses.iter().skip(i + 1) {
            match (entry_compete(a.metric), entry_compete(b.metric)) {
                (Some(x), Some(y)) if x == y => return true,
                _ => {}
            }
        }
    }
    false
}

fn exit_bags(cuts: &[Cut]) -> Vec<ExitBag> {
    let mut ranked: Vec<Cut> = cuts.to_vec();
    ranked.sort_by_key(|c| c.phase.menu_rank());
    // One representative cut per (metric, window) — prefer dump-lead / contrast / p90.
    let mut singles: Vec<Clause> = Vec::new();
    for c in &ranked {
        if singles.iter().any(|s| s.metric == c.metric && s.window == c.window) {
            continue;
        }
        let role = exit_role(c.metric);
        let already = singles.iter().filter(|s| exit_role(s.metric) == role).count();
        let cap = match role {
            ExitRole::Giveback | ExitRole::Clock => 2,
            ExitRole::Progress => 3,
            ExitRole::Flow => 8,
        };
        if already >= cap {
            continue;
        }
        singles.push(Clause::from(*c));
    }

    let mut bags = vec![ExitBag { clauses: vec![] }];
    for c in &singles {
        bags.push(ExitBag { clauses: vec![*c] });
    }
    for i in 0..singles.len() {
        for j in (i + 1)..singles.len() {
            if exit_competes(singles[i].metric, singles[j].metric) {
                continue;
            }
            bags.push(ExitBag {
                clauses: vec![singles[i], singles[j]],
            });
        }
    }
    bags
}

fn assemble(entry: &EntryFilling, exit: &ExitBag) -> RuleParams {
    let mut rp = RuleParams::default();
    if !entry.clauses.is_empty() {
        rp.entry = Some(side_from(&entry.clauses));
    }
    if !exit.clauses.is_empty() {
        rp.exit = Some(side_from(&exit.clauses));
    }
    rp
}

fn side_from(clauses: &[Clause]) -> SideConditions {
    let mut sc = SideConditions::default();
    for c in clauses {
        let instances = sc.0.entry(c.group).or_default();
        let gc = match instances
            .iter()
            .position(|g| same_window(g.strict_param("window_size_sec"), c.window))
        {
            Some(i) => &mut instances[i],
            None => {
                let mut g = GroupConditions::default();
                if let Some(w) = c.window {
                    g.strict.insert("window_size_sec".to_string(), w);
                }
                instances.push(g);
                instances.last_mut().expect("just pushed")
            }
        };
        gc.metrics
            .entry(c.metric)
            .or_default()
            .push(vec![Condition {
                operator: c.op,
                value: c.threshold,
            }]);
    }
    sc
}

fn same_window(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => false,
    }
}

/// Whether this filling is the empty-entry combo.
pub fn is_empty_entry(e: &EntryFilling) -> bool {
    e.clauses.is_empty()
}

pub fn same_exit_bag(a: &ExitBag, b: &ExitBag) -> bool {
    if a.clauses.len() != b.clauses.len() {
        return false;
    }
    a.clauses.iter().all(|c| {
        b.clauses.iter().any(|d| {
            d.metric == c.metric
                && d.group == c.group
                && same_window(d.window, c.window)
                && d.op == c.op
                && (d.threshold - c.threshold).abs() < 1e-9
        })
    })
}

/// Dynamic groups must carry a window; static groups must not.
pub fn clause_legal(c: &Clause) -> bool {
    match group_spec(c.group).kind {
        MetricKind::Dynamic => c.window.is_some(),
        MetricKind::Static => c.window.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_search::cuts::CutPhase;

    fn cut(group: MetricGroupId, metric: MetricId, op: Operator, v: f64) -> Cut {
        Cut {
            group,
            metric,
            window: None,
            op,
            threshold: v,
            phase: CutPhase::Peak,
        }
    }

    fn clause(group: MetricGroupId, metric: MetricId, op: Operator, v: f64) -> Clause {
        Clause {
            group,
            metric,
            window: None,
            op,
            threshold: v,
            phase: CutPhase::Peak,
        }
    }

    #[test]
    fn empty_entry_and_empty_exit_are_combos() {
        let table = CutTable {
            windows: vec![10.0],
            entry: vec![],
            exit: vec![],
            winner_fill: vec![],
        };
        let g = generate(&table);
        assert_eq!(g.len(), 1);
        assert!(g[0].entry.clauses.is_empty());
        assert!(g[0].exit.clauses.is_empty());
        assert!(g[0].params.entry.is_none());
        assert!(g[0].params.exit.is_none());
    }

    #[test]
    fn exit_bags_cap_at_two_and_respect_compete() {
        let table = CutTable {
            windows: vec![],
            entry: vec![],
            exit: vec![
                cut(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gte, 12.0),
                cut(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 15.0),
                cut(MetricGroupId::PriceLifetime, MetricId::Stall, Operator::Gte, 30.0),
            ],
            winner_fill: vec![],
        };
        let bags = exit_bags(&table.exit);
        assert!(bags.iter().any(|b| b.clauses.is_empty()));
        assert!(bags.iter().all(|b| b.clauses.len() <= 2));
        assert!(!bags.iter().any(|b| {
            b.clauses.len() == 2
                && b.clauses.iter().any(|c| c.metric == MetricId::Trail)
                && b.clauses.iter().any(|c| c.metric == MetricId::Retrace)
        }));
        assert!(bags.iter().any(|b| {
            b.clauses.len() == 2
                && b.clauses.iter().any(|c| c.metric == MetricId::Trail)
                && b.clauses.iter().any(|c| c.metric == MetricId::Stall)
        }));
    }

    #[test]
    fn extra_or_skips_compete_and_full_bags() {
        let base = GeneratedCombo {
            entry: EntryFilling { clauses: vec![] },
            exit: ExitBag {
                clauses: vec![clause(
                    MetricGroupId::PriceLifetime,
                    MetricId::Trail,
                    Operator::Gte,
                    12.0,
                )],
            },
            params: RuleParams::default(),
        };
        let table = CutTable {
            windows: vec![],
            entry: vec![],
            exit: vec![
                cut(MetricGroupId::Position, MetricId::Retrace, Operator::Gte, 15.0),
                cut(MetricGroupId::PriceLifetime, MetricId::Stall, Operator::Gte, 40.0),
            ],
            winner_fill: vec![],
        };
        let extra = extra_or_candidates(&base, &table);
        assert!(extra.iter().all(|g| !g.exit.clauses.iter().any(|c| c.metric == MetricId::Retrace)));
        assert!(extra.iter().any(|g| g.exit.clauses.iter().any(|c| c.metric == MetricId::Stall)));
    }

    #[test]
    fn same_window_clauses_merge() {
        let e = EntryFilling {
            clauses: vec![
                Clause {
                    group: MetricGroupId::FlowWindow,
                    metric: MetricId::Buy,
                    window: Some(10.0),
                    op: Operator::Gte,
                    threshold: 5.0,
                    phase: CutPhase::Peak,
                },
                Clause {
                    group: MetricGroupId::FlowWindow,
                    metric: MetricId::UniqueWallets,
                    window: Some(10.0),
                    op: Operator::Gte,
                    threshold: 8.0,
                    phase: CutPhase::Peak,
                },
            ],
        };
        let rp = assemble(&e, &ExitBag { clauses: vec![] });
        let side = rp.entry.expect("entry");
        let inst = &side.0[&MetricGroupId::FlowWindow];
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].strict_param("window_size_sec"), Some(10.0));
        assert!(inst[0].metrics.contains_key(&MetricId::Buy));
        assert!(inst[0].metrics.contains_key(&MetricId::UniqueWallets));
    }

    #[test]
    fn can_fail_pairs_time_and_liq() {
        let table = CutTable {
            windows: vec![],
            entry: vec![
                cut(MetricGroupId::Snapshot, MetricId::Time, Operator::Lt, 40.0),
                cut(MetricGroupId::Snapshot, MetricId::Liquidity, Operator::Gte, 20.0),
                Cut {
                    group: MetricGroupId::FlowWindow,
                    metric: MetricId::UniqueWallets,
                    window: Some(5.0),
                    op: Operator::Gte,
                    threshold: 8.0,
                    phase: CutPhase::Contrast,
                },
            ],
            exit: vec![],
            winner_fill: vec![],
        };
        let entries = entry_fillings(&table);
        assert!(entries.iter().any(|e| e.clauses.is_empty()));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 2
                && e.clauses.iter().any(|c| c.metric == MetricId::Time)
                && e.clauses.iter().any(|c| c.metric == MetricId::Liquidity)
        }));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 2
                && e.clauses.iter().any(|c| c.metric == MetricId::Liquidity)
                && e.clauses.iter().any(|c| c.metric == MetricId::UniqueWallets)
        }));
        assert!(entries.iter().all(|e| {
            e.clauses
                .iter()
                .filter(|c| {
                    matches!(
                        entry_role(c.metric),
                        Some(EntryRole::Selector) | Some(EntryRole::Extra)
                    )
                })
                .count()
                <= 2
        }));
    }

    #[test]
    fn retune_swaps_threshold_not_metric() {
        let base = GeneratedCombo {
            entry: EntryFilling { clauses: vec![] },
            exit: ExitBag {
                clauses: vec![clause(
                    MetricGroupId::PriceLifetime,
                    MetricId::Trail,
                    Operator::Gte,
                    12.0,
                )],
            },
            params: RuleParams::default(),
        };
        let table = CutTable {
            windows: vec![],
            entry: vec![],
            exit: vec![
                cut(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gte, 12.0),
                cut(MetricGroupId::PriceLifetime, MetricId::Trail, Operator::Gte, 18.0),
                cut(MetricGroupId::PriceLifetime, MetricId::Stall, Operator::Gte, 40.0),
            ],
            winner_fill: vec![],
        };
        let r = retune_candidates(&base, &table);
        assert!(r.iter().any(|g| {
            g.exit.clauses.len() == 1
                && g.exit.clauses[0].metric == MetricId::Trail
                && (g.exit.clauses[0].threshold - 18.0).abs() < 1e-9
        }));
        assert!(r.iter().all(|g| !g.exit.clauses.iter().any(|c| c.metric == MetricId::Stall)));
    }

    #[test]
    fn cooccur_drops_pairs_that_never_happen_together() {
        use std::collections::HashMap;
        let mut fill = HashMap::new();
        fill.insert(fill_key(MetricId::Time, None), 10.0);
        fill.insert(fill_key(MetricId::Liquidity, None), 80.0);
        let fills = vec![fill; 10];
        let time = Clause {
            group: MetricGroupId::Snapshot,
            metric: MetricId::Time,
            window: None,
            op: Operator::Lt,
            threshold: 5.0,
            phase: CutPhase::FillMoment,
        };
        let liq = Clause {
            group: MetricGroupId::Snapshot,
            metric: MetricId::Liquidity,
            window: None,
            op: Operator::Gte,
            threshold: 20.0,
            phase: CutPhase::FillMoment,
        };
        assert!(!pair_cooccurs(&time, &liq, &fills));
        let time_ok = Clause {
            group: MetricGroupId::Snapshot,
            metric: MetricId::Time,
            window: None,
            op: Operator::Lt,
            threshold: 40.0,
            phase: CutPhase::FillMoment,
        };
        assert!(pair_cooccurs(&time_ok, &liq, &fills));
        // Peak contrast is not gated on the 1.5× snapshot.
        let peak_time = clause(MetricGroupId::Snapshot, MetricId::Time, Operator::Lt, 5.0);
        let peak_liq = clause(MetricGroupId::Snapshot, MetricId::Liquidity, Operator::Gte, 20.0);
        assert!(pair_cooccurs(&peak_time, &peak_liq, &fills));
    }

    #[test]
    fn contrast_beats_fill_moment_on_same_metric() {
        let table = CutTable {
            windows: vec![],
            entry: vec![
                Cut {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 90.0,
                    phase: CutPhase::FillMoment,
                },
                Cut {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                },
                cut(MetricGroupId::Snapshot, MetricId::Time, Operator::Lt, 40.0),
            ],
            exit: vec![],
            winner_fill: vec![],
        };
        let entries = entry_fillings(&table);
        let liq_singles: Vec<&EntryFilling> = entries
            .iter()
            .filter(|e| e.clauses.len() == 1 && e.clauses[0].metric == MetricId::Liquidity)
            .collect();
        assert!(
            liq_singles
                .iter()
                .any(|e| (e.clauses[0].threshold - 47.0).abs() < 1e-9
                    && e.clauses[0].phase == CutPhase::Contrast),
            "peak contrast liq must be a filling"
        );
        assert!(
            liq_singles
                .iter()
                .all(|e| (e.clauses[0].threshold - 90.0).abs() > 1e-9),
            "fill-moment must not replace peak contrast for the same metric"
        );
    }

    #[test]
    fn retune_stays_same_phase() {
        let base = GeneratedCombo {
            entry: EntryFilling {
                clauses: vec![Clause {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                }],
            },
            exit: ExitBag { clauses: vec![] },
            params: RuleParams::default(),
        };
        let table = CutTable {
            windows: vec![],
            entry: vec![
                Cut {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                },
                Cut {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 55.0,
                    phase: CutPhase::Contrast,
                },
                Cut {
                    group: MetricGroupId::Snapshot,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 90.0,
                    phase: CutPhase::FillMoment,
                },
            ],
            exit: vec![],
            winner_fill: vec![],
        };
        let r = retune_candidates(&base, &table);
        assert!(r.iter().any(|g| {
            g.entry.clauses.len() == 1
                && (g.entry.clauses[0].threshold - 55.0).abs() < 1e-9
                && g.entry.clauses[0].phase == CutPhase::Contrast
        }));
        assert!(r.iter().all(|g| {
            !g.entry
                .clauses
                .iter()
                .any(|c| c.phase == CutPhase::FillMoment)
        }));
    }
}
