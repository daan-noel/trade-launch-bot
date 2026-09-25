//! Entry fillings × exit bags → complete `RuleParams` (`docs/arch/sweep.md` "Rule search").
//!
//! No kitchen-sink entry and no greedy add/drop. Empty entry and empty exit are
//! combos. Entry can-fail is 0–2 (selector + extra pooled); trigger stays 0–1.
//! Exit bags are 0–2 different metrics; giveback and clock still compete.

use std::collections::BTreeMap;

use hunter_engine::metrics::evaluator::{coalesce_contributions, Condition, Operator};
use hunter_engine::metrics::{metric_spec, MetricRef, Span};
use hunter_engine::rule_params::{Cond, Line, RuleParams, Sell};

use super::cuts::{cut_holds, fill_key, Cut, CutPhase, CutTable, MIN_SPLIT};
use super::roles::{
    entry_compete, entry_role, exit_competes, exit_role, quantity, EntryRole, ExitRole, TriggerFamily,
};

/// One authored clause (a read, side implied by which bag it sits in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clause {
    pub r: MetricRef,
    pub op: Operator,
    pub threshold: f64,
    pub phase: CutPhase,
}

impl From<Cut> for Clause {
    fn from(c: Cut) -> Self {
        Self { r: c.r, op: c.op, threshold: c.threshold, phase: c.phase }
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
    let used: Vec<_> = base.exit.clauses.iter().map(|c| quantity(&c.r)).collect();
    let mut ranked = cuts.exit.clone();
    ranked.sort_by_key(|c| c.phase.menu_rank());
    let mut out = Vec::new();
    for cut in &ranked {
        if used.contains(&quantity(&cut.r)) {
            continue;
        }
        if base.exit.clauses.iter().any(|c| exit_competes(&c.r, &cut.r)) {
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
    same_read(&a.r, &b.r)
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
        match entry_role(&c.r) {
            Some(EntryRole::Selector) | Some(EntryRole::Extra) => {
                // Floors are collected pre-dedup: a metric whose contrast cut
                // wins the singles menu still gets its p10..p90 band.
                if c.phase == CutPhase::WinnerFloor && !floors.iter().any(|f| same_read(&f.r, &c.r)) {
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
            .then_with(|| format!("{:?}", a.r.metric).cmp(&format!("{:?}", b.r.metric)))
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
            if quantity(&single.r) == quantity(&band[0].r) {
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
        if !seen.contains(&quantity(&c.r)) {
            seen.push(quantity(&c.r));
            primary.push(c);
        } else if c.phase.is_entry_extra() {
            extra.push(c);
        }
    }
    primary.truncate(CAN_FAIL_CAP);
    let mut extra_seen = Vec::new();
    extra.retain(|c| {
        if extra_seen.contains(&quantity(&c.r)) {
            false
        } else {
            extra_seen.push(quantity(&c.r));
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
        .filter(|m| match (m.get(&fill_key(&a.r)), m.get(&fill_key(&b.r))) {
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
            if base.exit.clauses.iter().enumerate().any(|(j, o)| j != i && exit_competes(&o.r, &n.r)) {
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
        .filter(|n| quantity(&n.r) == quantity(&c.r) && n.phase == c.phase)
        .filter(|n| !same_read(&n.r, &c.r) || n.op != c.op || (n.threshold - c.threshold).abs() > 1e-9)
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
            match (entry_compete(&a.r), entry_compete(&b.r)) {
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
    // One representative cut per read — prefer dump-lead / contrast / p90.
    let mut singles: Vec<Clause> = Vec::new();
    for c in &ranked {
        if singles.iter().any(|s| same_read(&s.r, &c.r)) {
            continue;
        }
        let role = exit_role(&c.r);
        let already = singles.iter().filter(|s| exit_role(&s.r) == role).count();
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
            if exit_competes(&singles[i].r, &singles[j].r) {
                continue;
            }
            bags.push(ExitBag {
                clauses: vec![singles[i], singles[j]],
            });
        }
    }
    bags
}

/// The rule a filling and a bag make: every entry clause an `enter.filters`
/// condition (AND), every exit clause its own `always` sell line (OR).
pub(crate) fn assemble(entry: &EntryFilling, exit: &ExitBag) -> RuleParams {
    let mut rp = RuleParams::default();
    rp.enter.filters = conds_from(&entry.clauses);
    rp.always = conds_from(&exit.clauses)
        .into_iter()
        .map(|c| Line { when: vec![c], sell: Some(Sell { label: None, pct: None }), go: None, off: false })
        .collect();
    rp
}

/// One condition per read, in first-seen order. Clauses on one read here are only
/// ever a band's floor + ceiling, which AND into one arm.
fn conds_from(clauses: &[Clause]) -> Vec<Cond> {
    let mut reads: Vec<(MetricRef, Vec<Condition>)> = Vec::new();
    for c in clauses {
        let cond = Condition { operator: c.op, value: c.threshold };
        match reads.iter_mut().find(|(r, _)| same_read(r, &c.r)) {
            Some((_, cs)) => cs.push(cond),
            None => reads.push((c.r, vec![cond])),
        }
    }
    reads
        .into_iter()
        .map(|(r, cs)| Cond::Metric { r, is: coalesce_contributions(cs, metric_spec(r.metric).eq_tolerance), off: false })
        .collect()
}

/// Human label for one authored clause — the ablation table's row key. The read
/// prints as the engine labels it (`m_flow.buy_sol @!volume [10s]`), so a row names
/// the same span a persisted exit reason and a live chip do.
pub fn clause_label(c: &Clause) -> String {
    format!("{} {} {} [{}]", c.r.label(), c.op.symbol(), c.threshold, c.phase.label())
}

/// Same metric, same tag, same span. Spans compare by their dedup identity, not the
/// raw floats: two clauses that differ in unit or lag read different tape and must
/// never merge into one condition, however close their sizes look.
pub(crate) fn same_read(a: &MetricRef, b: &MetricRef) -> bool {
    a.metric == b.metric && a.tag == b.tag && same_span(a.span, b.span)
}

fn same_span(a: Span, b: Span) -> bool {
    let key = |w: Option<hunter_engine::metrics::WindowSpec>| w.map(|w| w.key());
    key(a.window) == key(b.window) && key(a.slice) == key(b.slice) && a.since_age == b.since_age
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
            same_read(&d.r, &c.r) && d.op == c.op && (d.threshold - c.threshold).abs() < 1e-9
        })
    })
}

/// The metric accepts the read's tag and span. A since-age read carries an age
/// anchor the search cannot choose, so a clause on one is never legal here (the
/// rule editor authors it instead).
pub fn clause_legal(c: &Clause) -> bool {
    c.r.span.since_age.is_none() && c.r.check().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_search::cuts::CutPhase;
    use hunter_engine::metrics::{Metric, WindowSpec};

    fn life(m: Metric) -> MetricRef {
        MetricRef::life(m)
    }

    fn win(m: Metric, secs: f64) -> MetricRef {
        MetricRef::life(m).with_span(Span::window(WindowSpec::secs(secs)))
    }

    fn cut(r: MetricRef, op: Operator, v: f64) -> Cut {
        Cut { r, op, threshold: v, phase: CutPhase::Peak }
    }

    fn cut_at(r: MetricRef, op: Operator, v: f64, phase: CutPhase) -> Cut {
        Cut { r, op, threshold: v, phase }
    }

    fn clause(r: MetricRef, op: Operator, v: f64) -> Clause {
        Clause { r, op, threshold: v, phase: CutPhase::Peak }
    }

    fn table(entry: Vec<Cut>, exit: Vec<Cut>) -> CutTable {
        CutTable { entry, exit, ..CutTable::empty() }
    }

    fn has(clauses: &[Clause], m: Metric) -> bool {
        clauses.iter().any(|c| c.r.metric == m)
    }

    #[test]
    fn empty_entry_and_empty_exit_are_combos() {
        let table = CutTable { windows: vec![10.0], ..CutTable::empty() };
        let g = generate(&table);
        assert_eq!(g.len(), 1);
        assert!(g[0].entry.clauses.is_empty());
        assert!(g[0].exit.clauses.is_empty());
        assert!(g[0].params.enter.filters.is_empty());
        assert!(g[0].params.always.is_empty());
    }

    #[test]
    fn exit_bags_cap_at_two_and_respect_compete() {
        let table = table(
            vec![],
            vec![
                cut(life(Metric::TrailPct), Operator::Gte, 12.0),
                cut(life(Metric::RetracePct), Operator::Gte, 15.0),
                cut(life(Metric::StallSec), Operator::Gte, 30.0),
            ],
        );
        let bags = exit_bags(&table.exit);
        assert!(bags.iter().any(|b| b.clauses.is_empty()));
        assert!(bags.iter().all(|b| b.clauses.len() <= 2));
        assert!(!bags.iter().any(|b| {
            b.clauses.len() == 2 && has(&b.clauses, Metric::TrailPct) && has(&b.clauses, Metric::RetracePct)
        }));
        assert!(bags.iter().any(|b| {
            b.clauses.len() == 2 && has(&b.clauses, Metric::TrailPct) && has(&b.clauses, Metric::StallSec)
        }));
    }

    #[test]
    fn extra_or_skips_compete_and_full_bags() {
        let base = GeneratedCombo {
            entry: EntryFilling { clauses: vec![] },
            exit: ExitBag { clauses: vec![clause(life(Metric::TrailPct), Operator::Gte, 12.0)] },
            params: RuleParams::default(),
        };
        let table = table(
            vec![],
            vec![
                cut(life(Metric::RetracePct), Operator::Gte, 15.0),
                cut(life(Metric::StallSec), Operator::Gte, 40.0),
            ],
        );
        let extra = extra_or_candidates(&base, &table);
        assert!(extra.iter().all(|g| !has(&g.exit.clauses, Metric::RetracePct)));
        assert!(extra.iter().any(|g| has(&g.exit.clauses, Metric::StallSec)));
    }

    #[test]
    fn entry_clauses_are_filters_and_exit_clauses_sell_lines() {
        let e = EntryFilling {
            clauses: vec![
                clause(win(Metric::BuySol, 10.0), Operator::Gte, 5.0),
                clause(win(Metric::UniqueWallets, 10.0), Operator::Gte, 8.0),
            ],
        };
        let x = ExitBag {
            clauses: vec![
                clause(life(Metric::TrailPct), Operator::Gte, 20.0),
                clause(life(Metric::StallSec), Operator::Gte, 30.0),
            ],
        };
        let rp = assemble(&e, &x);
        assert_eq!(rp.enter.filters.len(), 2, "one AND condition per entry read");
        let reads: Vec<String> = rp.metric_refs().iter().map(MetricRef::label).collect();
        assert!(reads.contains(&"m_flow.buy_sol [10s]".to_string()), "{reads:?}");
        assert!(reads.contains(&"m_crowd.unique_wallets [10s]".to_string()), "{reads:?}");
        assert_eq!(rp.always.len(), 2, "one OR sell line per exit read");
        assert!(rp.always.iter().all(|l| l.when.len() == 1 && l.sell.is_some()));
        RuleParams::parse(&rp.to_value()).expect("the assembled rule passes the engine gate");
    }

    #[test]
    fn can_fail_pairs_time_and_liq() {
        let table = table(
            vec![
                cut(life(Metric::AgeSec), Operator::Lt, 40.0),
                cut(life(Metric::LiquiditySol), Operator::Gte, 20.0),
                cut_at(win(Metric::UniqueWallets, 5.0), Operator::Gte, 8.0, CutPhase::Contrast),
            ],
            vec![],
        );
        let entries = entry_fillings(&table);
        assert!(entries.iter().any(|e| e.clauses.is_empty()));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 2 && has(&e.clauses, Metric::AgeSec) && has(&e.clauses, Metric::LiquiditySol)
        }));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 2 && has(&e.clauses, Metric::LiquiditySol) && has(&e.clauses, Metric::UniqueWallets)
        }));
        assert!(entries.iter().all(|e| {
            e.clauses
                .iter()
                .filter(|c| matches!(entry_role(&c.r), Some(EntryRole::Selector) | Some(EntryRole::Extra)))
                .count()
                <= 2
        }));
    }

    #[test]
    fn retune_swaps_threshold_not_metric() {
        let base = GeneratedCombo {
            entry: EntryFilling { clauses: vec![] },
            exit: ExitBag { clauses: vec![clause(life(Metric::TrailPct), Operator::Gte, 12.0)] },
            params: RuleParams::default(),
        };
        let table = table(
            vec![],
            vec![
                cut(life(Metric::TrailPct), Operator::Gte, 12.0),
                cut(life(Metric::TrailPct), Operator::Gte, 18.0),
                cut(life(Metric::StallSec), Operator::Gte, 40.0),
            ],
        );
        let r = retune_candidates(&base, &table);
        assert!(r.iter().any(|g| {
            g.exit.clauses.len() == 1
                && g.exit.clauses[0].r.metric == Metric::TrailPct
                && (g.exit.clauses[0].threshold - 18.0).abs() < 1e-9
        }));
        assert!(r.iter().all(|g| !has(&g.exit.clauses, Metric::StallSec)));
    }

    #[test]
    fn cooccur_drops_pairs_that_never_happen_together() {
        use std::collections::HashMap;
        let mut fill = HashMap::new();
        fill.insert(fill_key(&life(Metric::AgeSec)), 10.0);
        fill.insert(fill_key(&life(Metric::LiquiditySol)), 80.0);
        let fills = vec![fill; 10];
        let table = CutTable { winner_fill: fills, ..CutTable::empty() };
        let at_fill = |r, op, v| Clause { r, op, threshold: v, phase: CutPhase::FillMoment };
        let time = at_fill(life(Metric::AgeSec), Operator::Lt, 5.0);
        let liq = at_fill(life(Metric::LiquiditySol), Operator::Gte, 20.0);
        assert!(!pair_cooccurs(&time, &liq, &table));
        let time_ok = at_fill(life(Metric::AgeSec), Operator::Lt, 40.0);
        assert!(pair_cooccurs(&time_ok, &liq, &table));
        let peak_time = clause(life(Metric::AgeSec), Operator::Lt, 5.0);
        let peak_liq = clause(life(Metric::LiquiditySol), Operator::Gte, 20.0);
        assert!(pair_cooccurs(&peak_time, &peak_liq, &table));
    }

    #[test]
    fn contrast_beats_fill_moment_on_same_metric() {
        let liq = life(Metric::LiquiditySol);
        let table = table(
            vec![
                cut_at(liq, Operator::Gte, 90.0, CutPhase::FillMoment),
                cut_at(liq, Operator::Gte, 47.0, CutPhase::Contrast),
                cut(life(Metric::AgeSec), Operator::Lt, 40.0),
            ],
            vec![],
        );
        let entries = entry_fillings(&table);
        let liq_singles: Vec<&EntryFilling> = entries
            .iter()
            .filter(|e| e.clauses.len() == 1 && e.clauses[0].r.metric == Metric::LiquiditySol)
            .collect();
        assert!(
            liq_singles
                .iter()
                .any(|e| (e.clauses[0].threshold - 47.0).abs() < 1e-9 && e.clauses[0].phase == CutPhase::Contrast),
            "peak contrast liq must be a filling"
        );
        assert!(
            liq_singles
                .iter()
                .any(|e| (e.clauses[0].threshold - 90.0).abs() < 1e-9 && e.clauses[0].phase == CutPhase::FillMoment),
            "fill-moment stays as an extra filling for the same metric"
        );
    }

    #[test]
    fn run_lead_is_kept_as_extra_beside_contrast() {
        let age = life(Metric::AgeSec);
        let table = table(
            vec![
                cut_at(age, Operator::Lt, 40.0, CutPhase::Contrast),
                cut_at(age, Operator::Lt, 8.0, CutPhase::RunLead),
                cut_at(life(Metric::LiquiditySol), Operator::Gte, 47.0, CutPhase::Contrast),
            ],
            vec![],
        );
        let entries = entry_fillings(&table);
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 1 && e.clauses[0].r.metric == Metric::AgeSec && e.clauses[0].phase == CutPhase::Contrast
        }));
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 1
                && e.clauses[0].r.metric == Metric::AgeSec
                && e.clauses[0].phase == CutPhase::RunLead
                && (e.clauses[0].threshold - 8.0).abs() < 1e-9
        }));
    }

    #[test]
    fn band_is_one_filling_and_one_and_arm() {
        let liq = life(Metric::LiquiditySol);
        let table = table(
            vec![
                cut_at(liq, Operator::Gte, 20.0, CutPhase::WinnerFloor),
                cut_at(liq, Operator::Lte, 70.0, CutPhase::WinnerCeil),
                cut(life(Metric::AgeSec), Operator::Lt, 40.0),
            ],
            vec![],
        );
        let entries = entry_fillings(&table);
        let band = entries
            .iter()
            .find(|e| e.clauses.len() == 2 && e.clauses.iter().all(|c| c.r.metric == Metric::LiquiditySol))
            .expect("floor + ceil band filling");
        // The band + one different-metric single occupies both can-fail slots.
        assert!(entries.iter().any(|e| {
            e.clauses.len() == 3
                && e.clauses.iter().filter(|c| c.r.metric == Metric::LiquiditySol).count() == 2
                && has(&e.clauses, Metric::AgeSec)
        }));
        // DNF: the band must assemble as ONE AND-arm, not floor OR ceil.
        let rp = assemble(band, &ExitBag { clauses: vec![] });
        assert_eq!(rp.enter.filters.len(), 1, "one condition on the one read");
        let Cond::Metric { is, .. } = &rp.enter.filters[0] else { panic!("a metric condition") };
        assert_eq!(is.len(), 1, "one OR-arm");
        assert_eq!(is[0].len(), 2, "floor AND ceil in that arm");
    }

    #[test]
    fn band_pair_is_not_a_compete_clash() {
        let floor = Clause {
            r: life(Metric::LiquiditySol),
            op: Operator::Gte,
            threshold: 20.0,
            phase: CutPhase::WinnerFloor,
        };
        let ceil = Clause { op: Operator::Lte, threshold: 70.0, phase: CutPhase::WinnerCeil, ..floor };
        assert!(is_band_pair(&floor, &ceil));
        assert!(!has_entry_compete_clash(&[floor, ceil]));
        // Two floors on one metric still clash.
        let floor2 = Clause { threshold: 30.0, ..floor };
        assert!(has_entry_compete_clash(&[floor, floor2]));
    }

    #[test]
    fn retune_stays_same_phase() {
        let liq = life(Metric::LiquiditySol);
        let base = GeneratedCombo {
            entry: EntryFilling {
                clauses: vec![Clause { r: liq, op: Operator::Gte, threshold: 47.0, phase: CutPhase::Contrast }],
            },
            exit: ExitBag { clauses: vec![] },
            params: RuleParams::default(),
        };
        let table = table(
            vec![
                cut_at(liq, Operator::Gte, 47.0, CutPhase::Contrast),
                cut_at(liq, Operator::Gte, 55.0, CutPhase::Contrast),
                cut_at(liq, Operator::Gte, 90.0, CutPhase::FillMoment),
            ],
            vec![],
        );
        let r = retune_candidates(&base, &table);
        assert!(r.iter().any(|g| {
            g.entry.clauses.len() == 1
                && (g.entry.clauses[0].threshold - 55.0).abs() < 1e-9
                && g.entry.clauses[0].phase == CutPhase::Contrast
        }));
        assert!(r.iter().all(|g| !g.entry.clauses.iter().any(|c| c.phase == CutPhase::FillMoment)));
    }
}
