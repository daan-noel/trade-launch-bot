//! Entry fillings × exit bags → complete `RuleParams` ([rule-search-method.md] §3).
//!
//! No kitchen-sink entry and no greedy add/drop. Empty entry and empty exit are
//! combos. Entry can-fail is 0–2 (selector + extra pooled); trigger stays 0–1.
//! Exit bags are 0–2 different metrics; giveback and clock still compete.

use std::collections::BTreeMap;

use hunter_engine::metrics::evaluator::{Condition, Operator};
use hunter_engine::metrics::{group_spec, MetricGroupId, MetricId, MetricKind};
use hunter_engine::rule_params::{GroupConditions, RuleParams, SideConditions};

use super::cuts::{cut_holds, fill_key, Cut, CutPhase, CutTable, MIN_SPLIT};
use super::roles::{
    entry_compete, entry_role, exit_competes, exit_role, EntryRole, ExitRole, TriggerFamily,
};

/// One authored clause (metric, side implied by which bag it sits in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clause {
    pub group: MetricGroupId,
    pub metric: MetricId,
    pub window: Option<hunter_engine::metrics::WindowSpec>,
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
const EXTRA_PHASE_CAP: usize = 6;
const RETUNE_CAP: usize = 12;
/// Cap on floor+ceiling band fillings in the product.
const BAND_CAP: usize = 6;

/// Two clauses forming a floor + ceiling on one quantity — the band counts as
/// ONE can-fail filling, never a same-metric compete clash.
pub fn is_band_pair(a: &Clause, b: &Clause) -> bool {
    a.metric == b.metric
        && a.group == b.group
        && same_window(a.window, b.window)
        && matches!(
            (a.op, b.op),
            (Operator::Gte | Operator::Gt, Operator::Lte | Operator::Lt)
                | (Operator::Lte | Operator::Lt, Operator::Gte | Operator::Gt)
        )
}

fn entry_fillings(cuts: &CutTable) -> Vec<EntryFilling> {
    let mut can_fail: Vec<Clause> = Vec::new();
    let mut ceils: Vec<Clause> = Vec::new();
    let mut floors: Vec<Clause> = Vec::new();
    let mut triggers: BTreeMap<TriggerFamily, Vec<Clause>> = BTreeMap::new();
    for c in &cuts.entry {
        // Ceilings exist only to pair with their floor into a band.
        if c.phase == CutPhase::WinnerCeil {
            ceils.push(Clause::from(*c));
            continue;
        }
        match entry_role(c.metric) {
            Some(EntryRole::Selector) | Some(EntryRole::Extra) => {
                // Floors are collected pre-dedup: a metric whose contrast cut
                // wins the singles menu still gets its p10..p90 band.
                if c.phase == CutPhase::WinnerFloor
                    && !floors
                        .iter()
                        .any(|f| f.metric == c.metric && same_window(f.window, c.window))
                {
                    floors.push(Clause::from(*c));
                }
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
    can_fail = take_can_fail(can_fail);

    let trigger_opts: Vec<Option<Clause>> = {
        let mut v = vec![None];
        for list in triggers.values() {
            let mut ranked = list.clone();
            ranked.sort_by_key(|c| c.phase.menu_rank());
            if let Some(c) = ranked.first() {
                v.push(Some(*c));
            }
            if let Some(c) = ranked.iter().find(|c| c.phase == CutPhase::RunLead) {
                if ranked.first().map(|f| f.phase) != Some(CutPhase::RunLead) {
                    v.push(Some(*c));
                }
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
            if !pair_cooccurs(&pair[0], &pair[1], cuts) {
                continue;
            }
            fail_opts.push(pair.to_vec());
        }
    }
    // Bands: floor (winner-floor) + its ceiling on one metric/window = ONE
    // can-fail filling — alone and beside one different-metric single (the
    // band occupies one of the two can-fail slots).
    let bands: Vec<[Clause; 2]> = floors
        .iter()
        .filter_map(|f| {
            ceils
                .iter()
                .find(|c| is_band_pair(f, c))
                .map(|c| [*f, *c])
        })
        .take(BAND_CAP)
        .collect();
    let mut band_pairs = 0usize;
    for band in &bands {
        fail_opts.push(band.to_vec());
        for single in &can_fail {
            if band_pairs >= BAND_CAP * 4 {
                break;
            }
            if single.metric == band[0].metric {
                continue;
            }
            let mut clauses = band.to_vec();
            clauses.push(*single);
            if has_entry_compete_clash(&clauses) {
                continue;
            }
            if !pair_cooccurs(&band[0], single, cuts) {
                continue;
            }
            fail_opts.push(clauses);
            band_pairs += 1;
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

fn take_can_fail(cuts: Vec<Clause>) -> Vec<Clause> {
    let mut primary = Vec::new();
    let mut extra = Vec::new();
    let mut seen = Vec::new();
    for c in cuts {
        if !seen.contains(&c.metric) {
            seen.push(c.metric);
            primary.push(c);
        } else if c.phase.is_entry_extra() {
            extra.push(c);
        }
    }
    primary.truncate(CAN_FAIL_CAP);
    let mut extra_seen = Vec::new();
    extra.retain(|c| {
        if extra_seen.contains(&c.metric) {
            false
        } else {
            extra_seen.push(c.metric);
            true
        }
    });
    extra.truncate(EXTRA_PHASE_CAP);
    primary.extend(extra);
    primary
}

fn pair_cooccurs(a: &Clause, b: &Clause, cuts: &CutTable) -> bool {
    // Same-clock extra pairs must co-occur on that clock. Peak contrast pairs
    // are not gated on a 1.5× snapshot.
    let snaps = match (a.phase, b.phase) {
        (CutPhase::RunLead, CutPhase::RunLead) => &cuts.winner_lead,
        (CutPhase::Launch, CutPhase::Launch) => &cuts.winner_launch,
        (CutPhase::FillMoment, CutPhase::FillMoment) => &cuts.winner_fill,
        _ => return true,
    };
    if snaps.len() < MIN_SPLIT {
        return true;
    }
    let n = snaps
        .iter()
        .filter(|m| match (m.get(&fill_key(a.metric, a.window)), m.get(&fill_key(b.metric, b.window))) {
            (Some(&va), Some(&vb)) => clause_holds(a, va) && clause_holds(b, vb),
            _ => false,
        })
        .count();
    n >= MIN_SPLIT
}

fn clause_holds(c: &Clause, v: f64) -> bool {
    cut_holds(c.op, c.threshold, v)
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
            if is_band_pair(a, b) {
                continue; // floor + ceiling on one quantity is ONE filling, not a clash
            }
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

pub(crate) fn assemble(entry: &EntryFilling, exit: &ExitBag) -> RuleParams {
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
            .position(|g| {
                same_window(g.window_spec(&hunter_engine::metrics::WINDOW_AXIS), c.window)
            })
        {
            Some(i) => &mut instances[i],
            None => {
                let mut g = GroupConditions::default();
                if let Some(w) = c.window {
                    // Write the param that matches the unit - writing seconds for a
                    // slot span would silently author a different rule.
                    g.strict.insert(w.unit.size_param().to_string(), w.size);
                    if w.lag > 0.0 {
                        g.strict
                            .insert(hunter_engine::metrics::WINDOW_LAG_PARAM.to_string(), w.lag);
                    }
                }
                instances.push(g);
                instances.last_mut().expect("just pushed")
            }
        };
        // The expr is DNF (OR of AND-arms). Same-metric clauses here are only
        // ever a band's floor + ceiling, which must AND — one arm, not two.
        let arms = gc.metrics.entry(c.metric).or_default();
        let cond = Condition {
            operator: c.op,
            value: c.threshold,
        };
        match arms.first_mut() {
            Some(arm) => arm.push(cond),
            None => arms.push(vec![cond]),
        }
    }
    sc
}

/// Human label for one authored clause — the ablation table's row key.
pub fn clause_label(c: &Clause) -> String {
    let op = match c.op {
        Operator::Gt => ">",
        Operator::Gte => ">=",
        Operator::Lt => "<",
        Operator::Lte => "<=",
        Operator::Eq => "=",
        Operator::Ne => "!=",
    };
    // Suffix from the engine's own `WindowUnit::suffix`, so an ablation row names the
    // same span a persisted exit reason and a live chip do. A lag prints only when
    // there is one - a lagged window reads different tape from an unlagged one of the
    // same size, and the two must not label identically.
    let window = c
        .window
        .map(|w| match w.lag > 0.0 {
            true => format!("({:.0}{}@{:.0})", w.size, w.unit.suffix(), w.lag),
            false => format!("({:.0}{})", w.size, w.unit.suffix()),
        })
        .unwrap_or_default();
    format!(
        "{} {}{window} {op} {} [{}]",
        c.group.name(),
        c.metric.name(),
        c.threshold,
        c.phase.label()
    )
}

fn same_window(a: Option<hunter_engine::metrics::WindowSpec>, b: Option<hunter_engine::metrics::WindowSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        // Compare the dedup identity, not the raw floats: two clauses that differ
        // in unit or lag read different tape and must never merge into one group
        // instance, however close their sizes look.
        (Some(x), Some(y)) => x.key() == y.key(),
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
            winner_lead: vec![],
            winner_launch: vec![],
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
            winner_lead: vec![],
            winner_launch: vec![],
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
            winner_lead: vec![],
            winner_launch: vec![],
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
                    window: Some(hunter_engine::metrics::WindowSpec::secs(10.0)),
                    op: Operator::Gte,
                    threshold: 5.0,
                    phase: CutPhase::Peak,
                },
                Clause {
                    group: MetricGroupId::FlowWindow,
                    metric: MetricId::UniqueWallets,
                    window: Some(hunter_engine::metrics::WindowSpec::secs(10.0)),
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
                cut(MetricGroupId::State, MetricId::Time, Operator::Lt, 40.0),
                cut(MetricGroupId::State, MetricId::Liquidity, Operator::Gte, 20.0),
                Cut {
                    group: MetricGroupId::FlowWindow,
                    metric: MetricId::UniqueWallets,
                    window: Some(hunter_engine::metrics::WindowSpec::secs(5.0)),
                    op: Operator::Gte,
                    threshold: 8.0,
                    phase: CutPhase::Contrast,
                },
            ],
            exit: vec![],
            winner_fill: vec![],
            winner_lead: vec![],
            winner_launch: vec![],
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
            winner_lead: vec![],
            winner_launch: vec![],
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
        let table = CutTable {
            winner_fill: fills,
            ..CutTable::empty()
        };
        let time = Clause {
            group: MetricGroupId::State,
            metric: MetricId::Time,
            window: None,
            op: Operator::Lt,
            threshold: 5.0,
            phase: CutPhase::FillMoment,
        };
        let liq = Clause {
            group: MetricGroupId::State,
            metric: MetricId::Liquidity,
            window: None,
            op: Operator::Gte,
            threshold: 20.0,
            phase: CutPhase::FillMoment,
        };
        assert!(!pair_cooccurs(&time, &liq, &table));
        let time_ok = Clause {
            group: MetricGroupId::State,
            metric: MetricId::Time,
            window: None,
            op: Operator::Lt,
            threshold: 40.0,
            phase: CutPhase::FillMoment,
        };
        assert!(pair_cooccurs(&time_ok, &liq, &table));
        let peak_time = clause(MetricGroupId::State, MetricId::Time, Operator::Lt, 5.0);
        let peak_liq = clause(MetricGroupId::State, MetricId::Liquidity, Operator::Gte, 20.0);
        assert!(pair_cooccurs(&peak_time, &peak_liq, &table));
    }

    #[test]
    fn contrast_beats_fill_moment_on_same_metric() {
        let table = CutTable {
            windows: vec![],
            entry: vec![
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 90.0,
                    phase: CutPhase::FillMoment,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                },
                cut(MetricGroupId::State, MetricId::Time, Operator::Lt, 40.0),
            ],
            exit: vec![],
            winner_fill: vec![],
            winner_lead: vec![],
            winner_launch: vec![],
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
                .any(|e| (e.clauses[0].threshold - 90.0).abs() < 1e-9
                    && e.clauses[0].phase == CutPhase::FillMoment),
            "fill-moment stays as an extra filling for the same metric"
        );
    }

    #[test]
    fn run_lead_is_kept_as_extra_beside_contrast() {
        let table = CutTable {
            windows: vec![],
            entry: vec![
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Time,
                    window: None,
                    op: Operator::Lt,
                    threshold: 40.0,
                    phase: CutPhase::Contrast,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Time,
                    window: None,
                    op: Operator::Lt,
                    threshold: 8.0,
                    phase: CutPhase::RunLead,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                },
            ],
            exit: vec![],
            winner_fill: vec![],
            winner_lead: vec![],
            winner_launch: vec![],
        };
        let entries = entry_fillings(&table);
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 1
                && e.clauses[0].metric == MetricId::Time
                && e.clauses[0].phase == CutPhase::Contrast
        }));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 1
                && e.clauses[0].metric == MetricId::Time
                && e.clauses[0].phase == CutPhase::RunLead
                && (e.clauses[0].threshold - 8.0).abs() < 1e-9
        }));
    }

    #[test]
    fn band_is_one_filling_and_one_and_arm() {
        let table = CutTable {
            windows: vec![],
            entry: vec![
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 20.0,
                    phase: CutPhase::WinnerFloor,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Lte,
                    threshold: 70.0,
                    phase: CutPhase::WinnerCeil,
                },
                cut(MetricGroupId::State, MetricId::Time, Operator::Lt, 40.0),
            ],
            exit: vec![],
            winner_fill: vec![],
            winner_lead: vec![],
            winner_launch: vec![],
        };
        let entries = entry_fillings(&table);
        let band = entries
            .iter()
            .find(|e| {
                e.clauses.len() == 2
                    && e.clauses.iter().all(|c| c.metric == MetricId::Liquidity)
            })
            .expect("floor + ceil band filling");
        // The band + one different-metric single occupies both can-fail slots.
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 3
                && e.clauses.iter().filter(|c| c.metric == MetricId::Liquidity).count() == 2
                && e.clauses.iter().any(|c| c.metric == MetricId::Time)
        }));
        // DNF: the band must assemble as ONE AND-arm, not floor OR ceil.
        let rp = assemble(band, &ExitBag { clauses: vec![] });
        let side = rp.entry.expect("entry");
        let inst = &side.0[&MetricGroupId::State];
        let arms = &inst[0].metrics[&MetricId::Liquidity];
        assert_eq!(arms.len(), 1, "one OR-arm");
        assert_eq!(arms[0].len(), 2, "floor AND ceil in that arm");
    }

    #[test]
    fn band_pair_is_not_a_compete_clash() {
        let floor = Clause {
            group: MetricGroupId::State,
            metric: MetricId::Liquidity,
            window: None,
            op: Operator::Gte,
            threshold: 20.0,
            phase: CutPhase::WinnerFloor,
        };
        let ceil = Clause {
            op: Operator::Lte,
            threshold: 70.0,
            phase: CutPhase::WinnerCeil,
            ..floor
        };
        assert!(is_band_pair(&floor, &ceil));
        assert!(!has_entry_compete_clash(&[floor, ceil]));
        // Two floors on one metric still clash.
        let floor2 = Clause {
            threshold: 30.0,
            ..floor
        };
        assert!(has_entry_compete_clash(&[floor, floor2]));
    }

    #[test]
    fn retune_stays_same_phase() {
        let base = GeneratedCombo {
            entry: EntryFilling {
                clauses: vec![Clause {
                    group: MetricGroupId::State,
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
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 47.0,
                    phase: CutPhase::Contrast,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 55.0,
                    phase: CutPhase::Contrast,
                },
                Cut {
                    group: MetricGroupId::State,
                    metric: MetricId::Liquidity,
                    window: None,
                    op: Operator::Gte,
                    threshold: 90.0,
                    phase: CutPhase::FillMoment,
                },
            ],
            exit: vec![],
            winner_fill: vec![],
            winner_lead: vec![],
            winner_launch: vec![],
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
