//! Sibling resolve — the family is the unit of a run, the fingerprint the unit of a
//! result (charter D1).
//!
//! Siblings share `ix_labels` and are **identical on every axis but one**. Purely
//! mechanical off the `fingerprints` table: no heuristic, no
//! fuzzy match, no similarity score. A cohort dominates a rule — one rule spans
//! −13.8% to +40.8% across six siblings — so a rule is never reportable without the
//! cohort it belongs to, and the exit is fitted across the family precisely because
//! exit logic is the half that transfers.

use anyhow::{bail, Result};
use uuid::Uuid;

use hunter_engine::fingerprint::{AxisId, AxisKind, AxisUnit};
use trading_core::models::Fingerprint;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;

/// The axes a family may vary. `ix_labels` is **not** here: siblings share the
/// launch shape by definition, so varying it produces a different launch style, not
/// a sibling of the same one.
///
/// A thin alias over the fingerprint axis registry rather than a parallel enum, so
/// an axis added there is varyable here without an edit — and so "the value on this
/// axis" can never mean one thing to a family and another to the matcher.
pub type Axis = AxisId;

/// Every varyable axis, in the one order ties resolve by — stable, so a family
/// resolve is reproducible run to run.
pub fn axes() -> impl Iterator<Item = Axis> {
    AxisId::ALL.into_iter().filter(|a| a.def().kind == AxisKind::Numeric)
}

/// This axis's value on a row, or `None` when the axis is not configured — which is
/// itself part of identity ("not part of this fingerprint").
///
/// Only an **exact** predicate has a value: a range spans a window, and a family is
/// ordered along one axis by where each member sits on it, which a window does not
/// answer. A range-valued axis therefore makes the two rows non-siblings rather than
/// silently collapsing to a bound.
pub fn axis_value(axis: Axis, fp: &Fingerprint) -> Option<u128> {
    fp.criteria.get(axis)?.as_exact()
}

/// The value a report prints: SOL for a lamports axis, the raw count otherwise.
pub fn display_value(axis: Axis, fp: &Fingerprint) -> Option<f64> {
    let raw = axis_value(axis, fp)?;
    Some(match axis.def().unit {
        AxisUnit::Lamports => raw as f64 / 1e9,
        _ => raw as f64,
    })
}

/// One member of a family — the target included.
#[derive(Clone, Debug, PartialEq)]
pub struct Sibling {
    pub fp_id: Uuid,
    pub name: String,
    /// The varied axis's value on this member (SOL for a lamports axis). `None` when
    /// the family varies nothing — a family of one.
    pub value: Option<f64>,
    /// Whether this member is the run's target: the held-out cohort the reported
    /// **level** comes from. The fit set is everyone else.
    pub is_target: bool,
}

/// A target fingerprint and its mechanically-resolved siblings.
#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub target: Uuid,
    /// The single axis the members differ on. `None` for a family of one — the run
    /// degrades to single-cohort and the report says so rather than inventing
    /// siblings.
    pub varied: Option<Axis>,
    /// Every member including the target, ordered by the varied axis's value
    /// (`None`s first), then by id — deterministic, so two runs list them alike.
    pub members: Vec<Sibling>,
}

impl Family {
    /// The fit set: every member except the target. Empty ⇔ [`Self::is_single`].
    pub fn fit_members(&self) -> impl Iterator<Item = &Sibling> {
        self.members.iter().filter(|m| !m.is_target)
    }

    /// Whether fit-broad does not apply here: the target has no sibling, so there is
    /// nothing to fit across and nothing to hold out.
    pub fn is_single(&self) -> bool {
        self.members.len() <= 1
    }

    /// The target's own row in `members`.
    pub fn target_member(&self) -> Option<&Sibling> {
        self.members.iter().find(|m| m.is_target)
    }
}

/// Whether two rows can be siblings at all: the same launch shape (`ix_labels`).
///
/// The retired form also compared the row-wide bucket width, because two rows
/// differing only in "bucketed at 0.1" vs "exact" matched different token
/// populations. There is no such row-wide mode now — a granularity difference shows
/// up as a differing predicate on the axis that carries it, which
/// [`differing_axes`] already counts.
fn shares_shape(a: &Fingerprint, b: &Fingerprint) -> bool {
    a.criteria.get(AxisId::IxLabels) == b.criteria.get(AxisId::IxLabels)
}

/// The axes on which `b` differs from `a`, comparing the predicates themselves — so
/// two rows naming the same amount at different granularities are correctly NOT the
/// same on that axis.
fn differing_axes(a: &Fingerprint, b: &Fingerprint) -> Vec<Axis> {
    axes().filter(|ax| a.criteria.get(*ax) != b.criteria.get(*ax)).collect()
}

/// Resolve the target's family out of a full fingerprint listing — the pure half, so
/// every family rule is testable with no DB.
///
/// `prefer` pins the varied axis when the caller already knows it. Left `None`, the
/// axis with the **most** siblings wins; a tie resolves by registry order, which is
/// arbitrary but fixed, so the same listing always yields the same family.
pub fn resolve_from(all: &[Fingerprint], target: Uuid, prefer: Option<Axis>) -> Result<Family> {
    let Some(t) = all.iter().find(|f| f.id == target) else {
        bail!("fingerprint {target} not found");
    };

    // Candidates: same shape, differing on exactly one axis.
    let mut by_axis: Vec<(Axis, Vec<&Fingerprint>)> =
        axes().map(|ax| (ax, Vec::new())).collect();
    for fp in all.iter().filter(|f| f.id != target && shares_shape(t, f)) {
        if let [only] = differing_axes(t, fp)[..] {
            // A sibling must actually sit somewhere ON the axis: a row that drops it
            // matches a different population rather than a point further along the
            // same dimension, and a row that RANGES over it names a window, which has
            // no position to order the family by.
            if axis_value(only, fp).is_some() && axis_value(only, t).is_some() {
                let slot =
                    by_axis.iter_mut().find(|(ax, _)| *ax == only).expect("axis is varyable");
                slot.1.push(fp);
            }
        }
    }

    let chosen = match prefer {
        // A pinned axis that varies nothing degrades to single-cohort; it never falls
        // back to a different family behind the caller's back.
        Some(ax) => by_axis.iter().find(|(a, _)| *a == ax).filter(|(_, m)| !m.is_empty()),
        None => {
            let best = by_axis.iter().map(|(_, m)| m.len()).max().unwrap_or(0);
            // First axis at the maximum, in registry order — a tie is arbitrary but
            // must be *fixed*, or the same listing could yield two different families.
            (best > 0).then(|| by_axis.iter().find(|(_, m)| m.len() == best)).flatten()
        }
    };

    let (varied, siblings): (Option<Axis>, &[&Fingerprint]) = match chosen {
        Some((ax, m)) => (Some(*ax), m.as_slice()),
        None => (None, &[]),
    };

    let mut members: Vec<Sibling> = std::iter::once(t)
        .chain(siblings.iter().copied())
        .map(|fp| Sibling {
            fp_id: fp.id,
            name: fp.name.clone(),
            value: varied.and_then(|ax| display_value(ax, fp)),
            is_target: fp.id == target,
        })
        .collect();
    members.sort_by(|a, b| {
        a.value
            .partial_cmp(&b.value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.fp_id.cmp(&b.fp_id))
    });
    Ok(Family { target, varied, members })
}

/// [`resolve_from`] against the live `fingerprints` table. The table is a small
/// dimension (tens of rows), so one `list` is cheaper than seven indexed probes and
/// keeps the whole family rule in one pure function.
pub async fn resolve(repo: &FingerprintRepo, target: Uuid, prefer: Option<Axis>) -> Result<Family> {
    let all = repo.list().await?;
    resolve_from(&all, target, prefer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::fixtures::fp_row;
    use hunter_engine::fingerprint::AxisPredicate;
    use trading_core::config::constants::sol_to_lamports;

    fn spend(name: &str, sol: f64) -> Fingerprint {
        let mut f = fp_row(name);
        f.criteria
            .insert(AxisId::SpendableLamportsIn, AxisPredicate::exact(sol_to_lamports(sol) as u128));
        f
    }

    /// The charter's reference family: six `3ix:BuyExactSolIn · bkt=exact` rows
    /// varying `spendable_lamports_in`.
    fn reference_family() -> Vec<Fingerprint> {
        [1.0, 1.5, 2.0, 3.0, 4.0, 5.0]
            .into_iter()
            .map(|s| spend(&format!("spend={s}"), s))
            .collect()
    }

    #[test]
    fn siblings_are_every_row_identical_but_one_axis() {
        let all = reference_family();
        let target = all[5].id; // spend=5
        let fam = resolve_from(&all, target, None).expect("resolved");

        assert_eq!(fam.varied, Some(Axis::SpendableLamportsIn));
        assert_eq!(fam.members.len(), 6);
        assert_eq!(fam.fit_members().count(), 5, "the target is held out of the fit set");
        // Ordered by the varied axis, so the report reads like the charter's table.
        let values: Vec<f64> = fam.members.iter().map(|m| m.value.unwrap()).collect();
        assert_eq!(values, vec![1.0, 1.5, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(fam.target_member().unwrap().value, Some(5.0));
        assert!(!fam.is_single());
    }

    #[test]
    fn a_different_shape_is_not_a_sibling() {
        let mut all = reference_family();
        let target = all[0].id;

        // Same spend axis, DIFFERENT ix_labels — a different launch style entirely.
        let mut other_labels = spend("other-labels", 9.0);
        other_labels.criteria.insert(
            AxisId::IxLabels,
            AxisPredicate::Sequence { labels: vec!["Pump.Fun: Create".into()] },
        );
        all.push(other_labels);

        // Same axis, but RANGED instead of pinned — a window has no position to order
        // the family by, so it is not a point further along the same dimension.
        let mut ranged = spend("ranged", 9.5);
        ranged.criteria.insert(
            AxisId::SpendableLamportsIn,
            AxisPredicate::range(Some(9_500_000_000), Some(9_600_000_000)),
        );
        all.push(ranged);

        // Same labels and width, but differs on TWO axes — not a sibling either.
        let mut two_axes = spend("two-axes", 9.9);
        two_axes.criteria.insert(AxisId::CuPrice, AxisPredicate::exact(777));
        all.push(two_axes);

        let fam = resolve_from(&all, target, None).expect("resolved");
        assert_eq!(fam.members.len(), 6, "only the reference six");
        assert!(fam.members.iter().all(|m| m.value.unwrap() <= 5.0));
    }

    #[test]
    fn a_family_of_one_degrades_to_single_cohort() {
        // A lone row with nothing else in the table to be a sibling of.
        let all = vec![spend("lonely", 2.0)];
        let fam = resolve_from(&all, all[0].id, None).expect("resolved");
        assert!(fam.is_single(), "a family of one is a valid outcome, not an error");
        assert_eq!(fam.varied, None, "nothing varies, so no axis is invented");
        assert_eq!(fam.fit_members().count(), 0);
        assert_eq!(fam.members.len(), 1);
        assert!(fam.members[0].is_target);
    }

    #[test]
    fn a_dropped_axis_is_a_different_population_not_a_sibling() {
        let mut all = reference_family();
        let target = all[0].id;
        // Same shape, but the spend axis is simply absent: that row matches every
        // spendable value, so it is not one point along the family's dimension.
        let mut unset = spend("unset", 0.0);
        unset.criteria.remove(AxisId::SpendableLamportsIn);
        all.push(unset);

        let fam = resolve_from(&all, target, None).expect("resolved");
        assert_eq!(fam.members.len(), 6);
    }

    #[test]
    fn the_varied_axis_can_be_pinned_and_ties_resolve_deterministically() {
        // Two candidate families off one target: 2 siblings on cu_price, 2 on spend.
        let base = spend("target", 2.0);
        let target = base.id;
        let mut all = vec![base];
        for p in [100i64, 200] {
            // Same spend as the target; only `cu_price` moves.
            let mut f = spend(&format!("cu{p}"), 2.0);
            f.criteria.insert(AxisId::CuPrice, AxisPredicate::exact(p as u128));
            all.push(f);
        }
        for s in [3.0, 4.0] {
            all.push(spend(&format!("spend{s}"), s));
        }

        // Unpinned: a tie lands on the earlier axis in `AXES` (cu_price), and lands
        // there every time — never on whichever the listing happened to yield last.
        let auto = resolve_from(&all, target, None).expect("resolved");
        assert_eq!(auto.varied, Some(Axis::CuPrice));
        assert_eq!(resolve_from(&all, target, None).unwrap(), auto, "reproducible");

        // Pinned: the caller's axis wins.
        let pinned = resolve_from(&all, target, Some(Axis::SpendableLamportsIn)).expect("resolved");
        assert_eq!(pinned.varied, Some(Axis::SpendableLamportsIn));
        assert_eq!(pinned.members.len(), 3);

        // Pinning an axis nothing varies on degrades to single-cohort, it does not
        // fall back to a different family behind the caller's back.
        let none = resolve_from(&all, target, Some(Axis::MaxCostLamports)).expect("resolved");
        assert!(none.is_single());
    }

    #[test]
    fn an_unknown_target_is_an_error_not_an_empty_family() {
        let all = reference_family();
        assert!(resolve_from(&all, Uuid::nil(), None).is_err());
    }
}
