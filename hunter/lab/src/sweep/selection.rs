//! **The one derivation of what a grouped-sweep group actually selected.**
//!
//! A group's token set is `corpus-window ∧ (scope-fingerprint | manual filters) ∧
//! group_key@precision`. Every one of those clauses is persisted on the run row or
//! the group row, but they used to be re-assembled ad hoc at each consumer —
//! `promote_group` rebuilt a fingerprint from `group_key` alone (dropping
//! `field_filters`), and the frontend rebuilt a *second*, differently-lossy version
//! in TypeScript to badge the group card. Two readers reconstructing one fact they
//! were never handed, both failing in the **widening** direction: the promoted rule
//! armed on a superset of the tokens the numbers came from.
//!
//! This module is that fact, resolved once ([`GroupSelection::resolve`]) and
//! consumed by everything: promote materializes it into a fingerprint
//! ([`GroupSelection::materialize`]), the groups API serializes it for display.
//!
//! **Fail closed.** A fingerprint axis is a scalar plus one *fingerprint-wide*
//! bucket width, so it cannot express every clause a sweep can select on (an
//! absent axis, a multi-value pin, a `u64` ceiling, the two axes the fingerprint
//! has no column for, or two axes needing different widths). Where the selection
//! doesn't fit, [`materialize`](GroupSelection::materialize) returns the blocking
//! clauses so the caller can refuse — it never drops a clause and hands back a
//! wider fingerprint. Widening that coverage means giving the axis its own
//! predicate (per-axis `eq | range | in | absent`), not loosening this seam.

use std::fmt::Write as _;

use hunter_engine::grouping::{
    bucket_index, exact_sol_label, exact_sol_label_u64, normalize_labels, GroupField, SolFilter,
    SolPrecision, BUCKET_EPS, LAMPORTS_PER_SOL_F64,
};
use serde::Serialize;
use serde_json::Value;
use trading_core::config::constants::tidy_sol_decimal;
use trading_core::models::Fingerprint;
use uuid::Uuid;

use crate::models::grouped_sweep::GroupedSweepRun;

/// The `∅` group-key sentinel: the token has no value for this axis. Mirrors
/// `hunter_engine::grouping`'s private `MISSING` (it is a wire format — the key
/// string is what the sweep persisted and the dashboard renders).
const MISSING: &str = "∅";

/// Where a clause came from. Kept on every clause so an error message can name
/// the thing the user set ("the run's field filter", not "an axis").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// An axis of the saved fingerprint the run was scoped to.
    Scope,
    /// A manual `ix_labels_filter` / `field_filters` entry on the run.
    Filter,
    /// A `group_by` axis — this group's own key.
    GroupBy,
}

impl Origin {
    fn describe(self) -> &'static str {
        match self {
            Origin::Scope => "scope fingerprint",
            Origin::Filter => "run filter",
            Origin::GroupBy => "group-by",
        }
    }
}

/// What one axis is pinned to. The SOL-amount variants carry **lamports**, the
/// unit every fingerprint axis and every matcher compare uses.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AxisPredicate {
    /// Exact integer (`cu_limit` / `cu_price`).
    Int(i64),
    /// Exact text (`token_program_id`).
    Text(String),
    /// Exact boolean (`is_cashback_enabled`).
    Bool(bool),
    /// Exact ordered instruction-label sequence — same semantics on both sides
    /// (the sweep's set filter and the engine's `ix_labels` axis are both an
    /// exact ordered sequence), so this maps across losslessly.
    Labels(Vec<String>),
    /// One exact lamports amount.
    Lamports(i64),
    /// Half-open `[lo, hi)` lamports — a bucket key, or a range filter entry.
    LamportsRange { lo: i64, hi: i64 },
    /// Two or more alternatives, verbatim as the filter spelled them. A
    /// fingerprint axis holds one value, so this never materializes.
    AnyOf(Vec<String>),
    /// An amount past `i64` — pump.fun's `u64::MAX` "no slippage cap" ceiling.
    /// A *sentinel, not an amount*: it has no `BIGINT` axis to live in.
    Ceiling(u64),
    /// The axis is **absent** on this group's tokens (the `∅` key). Distinct from
    /// "unconstrained": a `None` fingerprint axis matches tokens that *have* a
    /// value, which is the opposite of what this group selected.
    Absent,
}

impl AxisPredicate {
    /// Human-readable value, for the group card and for error messages.
    pub fn display(&self) -> String {
        match self {
            AxisPredicate::Int(v) => v.to_string(),
            AxisPredicate::Text(s) => s.clone(),
            AxisPredicate::Bool(b) => b.to_string(),
            AxisPredicate::Labels(l) => l.join(" | "),
            AxisPredicate::Lamports(v) => exact_sol_label(*v),
            AxisPredicate::LamportsRange { lo, hi } => {
                format!("{}–{}", exact_sol_label(*lo), exact_sol_label(*hi))
            }
            AxisPredicate::AnyOf(vs) => format!("any of [{}]", vs.join(", ")),
            AxisPredicate::Ceiling(v) => format!("{} (no cap)", exact_sol_label_u64(*v)),
            AxisPredicate::Absent => "∅ (absent)".to_string(),
        }
    }
}

/// One axis of the selection: what it is pinned to, and who pinned it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clause {
    /// Serde tag of the axis (`"max_cost_lamports"`, …) — the key the group_key,
    /// the `field_filters` map and the frontend column labels all already use.
    pub field: &'static str,
    pub predicate: AxisPredicate,
    pub origin: Origin,
    /// Pre-rendered `predicate.display()`, so the frontend never re-derives it.
    pub display: String,
}

/// Everything that selected one group's tokens, in one canonical vocabulary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GroupSelection {
    /// The saved fingerprint the run was scoped to, if any. Kept for provenance —
    /// the scope's axes are already expanded into `clauses`.
    pub scope_fingerprint_id: Option<Uuid>,
    /// Axis clauses, in [`GroupField`] declaration order (stable for display and
    /// for the `find_or_create` identity that follows from it).
    pub clauses: Vec<Clause>,
    /// Whether [`materialize`](Self::materialize) would succeed — precomputed so
    /// the group card can gray out Promote instead of failing on click.
    pub promotable: bool,
    /// Why not, when `promotable` is false. Empty otherwise.
    pub blockers: Vec<String>,
    /// The identity fields of the fingerprint this group promotes to — exactly the
    /// columns `FingerprintRepo::find_or_create` keys on. `None` when not
    /// promotable.
    ///
    /// Emitted so the frontend can tell whether that fingerprint already exists by
    /// *comparing* an identity the backend authored, instead of rebuilding one from
    /// the group key in TypeScript (a second derivation, and the reason a filtered
    /// run's card could never badge its own fingerprint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Value>,
}

/// Axis order for [`GroupSelection::clauses`] — the fingerprint's own field order,
/// so a selection reads like the rule editor.
const FIELD_ORDER: [GroupField; 10] = [
    GroupField::IxLabels,
    GroupField::CuLimit,
    GroupField::CuPrice,
    GroupField::InitialBuySol,
    GroupField::MaxCostLamports,
    GroupField::SpendableLamportsIn,
    GroupField::FirstSlotBuySol,
    GroupField::FirstSlotSellSol,
    GroupField::TokenProgramId,
    GroupField::IsCashbackEnabled,
];

impl GroupSelection {
    /// Resolve `run` + this group's `group_key` into the canonical selection.
    ///
    /// `scope_fp` must be the run's `fingerprint_id` row when it has one (the
    /// caller owns the fetch); its axes are expanded into clauses so a scoped run
    /// and a filter run produce the *same* vocabulary downstream. A group-by axis
    /// wins over a scope/filter clause on the same field: it is this group's own
    /// slice, always at least as narrow as what selected the corpus.
    ///
    /// Legacy rows need no special case — every input is a persisted run column,
    /// so a run swept before this module existed resolves identically.
    pub fn resolve(
        run: &GroupedSweepRun,
        scope_fp: Option<&Fingerprint>,
        group_key: &Value,
    ) -> Self {
        let mut acc: Vec<Clause> = Vec::new();

        // 1. The scope fingerprint's own axes, at the fingerprint's precision (NOT
        //    the run's — identity is the fingerprint's, and the two can differ).
        if let Some(fp) = scope_fp {
            let p = SolPrecision::from_width(fp.bucket_size_amount);
            push(&mut acc, GroupField::CuLimit, fp.cu_limit.map(AxisPredicate::Int), Origin::Scope);
            push(&mut acc, GroupField::CuPrice, fp.cu_price.map(AxisPredicate::Int), Origin::Scope);
            push(
                &mut acc,
                GroupField::IxLabels,
                fp.ix_labels.clone().filter(|l| !l.is_empty()).map(AxisPredicate::Labels),
                Origin::Scope,
            );
            for (field, v) in [
                (GroupField::InitialBuySol, fp.init_buy_lamports),
                (GroupField::MaxCostLamports, fp.max_cost_lamports),
                (GroupField::SpendableLamportsIn, fp.spendable_lamports_in),
                (GroupField::FirstSlotBuySol, fp.first_slot_buy_lamports),
                (GroupField::FirstSlotSellSol, fp.first_slot_sell_lamports),
            ] {
                push(&mut acc, field, v.map(|l| lamports_at(l, p)), Origin::Scope);
            }
        }

        // 2. The manual filters. Mutually exclusive with the scope by construction
        //    (`insert_run` nulls them on a scoped run), but resolved unconditionally
        //    so a hand-written row can never quietly lose a clause.
        let labels = run
            .ix_labels_filter
            .as_ref()
            .map(normalize_labels)
            .filter(|l| !l.is_empty())
            .map(AxisPredicate::Labels);
        push(&mut acc, GroupField::IxLabels, labels, Origin::Filter);
        if let Some(map) = run.field_filters.as_ref().and_then(Value::as_object) {
            for (tag, vals) in map {
                let Some(field) = GroupField::from_tag(tag) else { continue };
                let Some(vals) = vals.as_array().filter(|v| !v.is_empty()) else { continue };
                push(&mut acc, field, Some(filter_predicate(field, vals)), Origin::Filter);
            }
        }

        // 3. This group's own key, at the run's grouping precision. Overrides the
        //    above: the group is a sub-slice of whatever selected the corpus.
        if let Some(obj) = group_key.as_object() {
            for (tag, val) in obj {
                let Some(field) = GroupField::from_tag(tag) else { continue };
                let Some(s) = val.as_str() else { continue };
                push(&mut acc, field, Some(key_predicate(field, s)), Origin::GroupBy);
            }
        }

        acc.sort_by_key(|c| FIELD_ORDER.iter().position(|f| f.as_str() == c.field).unwrap_or(99));
        let mut sel = GroupSelection {
            scope_fingerprint_id: run.fingerprint_id,
            clauses: acc,
            promotable: false,
            blockers: Vec::new(),
            identity: None,
        };
        match sel.materialize(String::new()) {
            Ok(fp) => {
                sel.promotable = true;
                sel.identity = Some(serde_json::json!({
                    "cu_limit": fp.cu_limit,
                    "cu_price": fp.cu_price,
                    "init_buy_lamports": fp.init_buy_lamports,
                    "max_cost_lamports": fp.max_cost_lamports,
                    "spendable_lamports_in": fp.spendable_lamports_in,
                    "first_slot_buy_lamports": fp.first_slot_buy_lamports,
                    "first_slot_sell_lamports": fp.first_slot_sell_lamports,
                    "bucket_size_amount": fp.bucket_size_amount,
                    "ix_labels": fp.ix_labels,
                }));
            }
            Err(b) => sel.blockers = b,
        }
        sel
    }

    /// Build the fingerprint that matches **exactly** this selection, or return
    /// every clause that has no fingerprint expression.
    ///
    /// The returned fingerprint carries no `metric_config` — the caller owns that
    /// (the scope fingerprint's config / the run's volume-ix patterns).
    pub fn materialize(&self, name: String) -> Result<Fingerprint, Vec<String>> {
        let mut blockers: Vec<String> = Vec::new();
        let mut fp = Fingerprint {
            id: Uuid::new_v4(),
            name,
            cu_limit: None,
            cu_price: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: None,
            ix_labels: None,
            metric_config: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // A fingerprint carries ONE width for every SOL axis, so the axes must agree
        // on a precision before any of them can be written. `None` = exact mode.
        let mut width: Option<Option<f64>> = None;
        let mut sol: Vec<(&Clause, i64)> = Vec::new();

        for c in &self.clauses {
            let Some(field) = GroupField::from_tag(c.field) else { continue };
            match (&c.predicate, field) {
                (AxisPredicate::Int(v), GroupField::CuLimit) => fp.cu_limit = Some(*v),
                (AxisPredicate::Int(v), GroupField::CuPrice) => fp.cu_price = Some(*v),
                (AxisPredicate::Labels(l), GroupField::IxLabels) => fp.ix_labels = Some(l.clone()),
                (AxisPredicate::Lamports(v), _) if field.is_bucketed() => {
                    reconcile(&mut width, None, c, &mut blockers);
                    sol.push((c, *v));
                }
                (AxisPredicate::LamportsRange { lo, hi }, _) if field.is_bucketed() => {
                    match bucket_width_of(*lo, *hi) {
                        Some(w) => {
                            reconcile(&mut width, Some(w), c, &mut blockers);
                            sol.push((c, *lo));
                        }
                        // A range that isn't a bucket at its own width (an
                        // arbitrary `1.5–1.7` filter, or one not aligned to a
                        // multiple of the width) has no anchor+width spelling.
                        None => blockers.push(blocked(
                            c,
                            "an arbitrary range — a fingerprint SOL axis is one value plus the \
                             fingerprint-wide bucket width",
                        )),
                    }
                }
                (AxisPredicate::Ceiling(_), _) => blockers.push(blocked(
                    c,
                    "a \"no cap\" ceiling past i64 — it is a sentinel, not an amount, and no \
                     BIGINT axis can hold it",
                )),
                (AxisPredicate::Absent, _) => blockers.push(blocked(
                    c,
                    "an absent axis — a fingerprint spells \"unset\" as \"unconstrained\", which \
                     would also match tokens that HAVE a value",
                )),
                (AxisPredicate::AnyOf(_), _) => blockers.push(blocked(
                    c,
                    "several alternatives — a fingerprint axis holds exactly one value",
                )),
                (_, GroupField::TokenProgramId) => blockers.push(blocked(
                    c,
                    "the fingerprint has no token-program axis",
                )),
                (_, GroupField::IsCashbackEnabled) => blockers.push(blocked(
                    c,
                    "the fingerprint has no cashback axis",
                )),
                // A predicate that doesn't fit its own field (a hand-edited row).
                (p, f) => blockers.push(blocked(
                    c,
                    &format!("{p:?} is not a valid predicate for {}", f.as_str()),
                )),
            }
        }

        let resolved_width = width.flatten();
        for (c, lamports) in sol {
            let Some(field) = GroupField::from_tag(c.field) else { continue };
            match field {
                GroupField::InitialBuySol => fp.init_buy_lamports = Some(lamports),
                GroupField::MaxCostLamports => fp.max_cost_lamports = Some(lamports),
                GroupField::SpendableLamportsIn => fp.spendable_lamports_in = Some(lamports),
                GroupField::FirstSlotBuySol => fp.first_slot_buy_lamports = Some(lamports),
                GroupField::FirstSlotSellSol => fp.first_slot_sell_lamports = Some(lamports),
                _ => {}
            }
        }
        fp.bucket_size_amount = resolved_width.map(tidy_sol_decimal);

        // `matches` refuses an all-`None` fingerprint (it would arm on everything),
        // so an unexpressible-only selection must fail here rather than create a
        // row that silently matches nothing.
        let armed = trading_core::strategies::fingerprint_axes::fp_to_engine(&fp);
        if blockers.is_empty() && !armed.has_any_criterion() {
            blockers.push(
                "this group has no fingerprint-expressible criterion — the rule would have no \
                 entry gate at all"
                    .to_string(),
            );
        }
        if blockers.is_empty() {
            Ok(fp)
        } else {
            Err(blockers)
        }
    }

    /// Whether every clause came from the scope fingerprint — i.e. the group is
    /// the whole scope, with no group-by or filter narrowing on top. The promote
    /// path reuses the saved row itself in that case instead of materializing a
    /// match-identical twin (a bucketed axis re-anchors on its bucket's lower
    /// edge, which matches the same tokens but is a different `find_or_create`
    /// identity — and a duplicate fingerprint in the library is noise).
    pub fn is_scope_only(&self) -> bool {
        self.scope_fingerprint_id.is_some()
            && !self.clauses.is_empty()
            && self.clauses.iter().all(|c| c.origin == Origin::Scope)
    }

    /// One-line summary for logs / the run header (`"ix_labels = … · max cost = 0.324"`).
    pub fn summary(&self) -> String {
        if self.clauses.is_empty() {
            return "every token in the corpus window".to_string();
        }
        let mut s = String::new();
        for (i, c) in self.clauses.iter().enumerate() {
            if i > 0 {
                s.push_str(" · ");
            }
            let _ = write!(s, "{} = {}", c.field, c.display);
        }
        s
    }
}

/// Append a clause, replacing any earlier one for the same field (later stages —
/// filter, then group-by — are narrower than earlier ones by construction).
fn push(acc: &mut Vec<Clause>, field: GroupField, pred: Option<AxisPredicate>, origin: Origin) {
    let Some(predicate) = pred else { return };
    let display = predicate.display();
    let clause = Clause { field: field.as_str(), predicate, origin, display };
    match acc.iter_mut().find(|c| c.field == field.as_str()) {
        Some(slot) => *slot = clause,
        None => acc.push(clause),
    }
}

/// A stored fingerprint axis → predicate, at the fingerprint's own precision.
fn lamports_at(lamports: i64, precision: SolPrecision) -> AxisPredicate {
    match precision {
        SolPrecision::Exact => AxisPredicate::Lamports(lamports),
        SolPrecision::Bucket(w) => {
            let lo = bucket_index(lamports as f64 / LAMPORTS_PER_SOL_F64, w) as f64 * w;
            AxisPredicate::LamportsRange {
                lo: sol_to_lamports_round(lo),
                hi: sol_to_lamports_round(lo + w),
            }
        }
    }
}

/// A `field_filters` entry → predicate. One entry parses to an exact amount or a
/// range through the shared [`SolFilter`] (the same parser the corpus filter used,
/// so the selection can't disagree with what was actually swept); two or more stay
/// verbatim as [`AxisPredicate::AnyOf`].
fn filter_predicate(field: GroupField, vals: &[Value]) -> AxisPredicate {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if vals.len() > 1 {
        return AxisPredicate::AnyOf(vals.iter().map(text).collect());
    }
    let v = &vals[0];
    if field.is_bucketed() {
        return match SolFilter::parse(&text(v)) {
            Some(SolFilter::Exact(l)) => AxisPredicate::Lamports(l),
            Some(SolFilter::Range(lo, hi)) => AxisPredicate::LamportsRange { lo, hi },
            // Unparseable: the corpus filter dropped every token, so nothing
            // reached this group — but keep it visible rather than silently gone.
            None => AxisPredicate::AnyOf(vec![text(v)]),
        };
    }
    match field {
        GroupField::CuLimit | GroupField::CuPrice => match v.as_i64() {
            Some(i) => AxisPredicate::Int(i),
            None => AxisPredicate::AnyOf(vec![text(v)]),
        },
        GroupField::IsCashbackEnabled => match v.as_bool() {
            Some(b) => AxisPredicate::Bool(b),
            None => AxisPredicate::AnyOf(vec![text(v)]),
        },
        _ => AxisPredicate::Text(text(v)),
    }
}

/// A `group_key` label → predicate. The inverse of `hunter_engine::grouping`'s
/// `render_field`: `∅` is the absent sentinel, a bucketed axis renders `"lo–hi"`
/// (or the exact amount in `SolPrecision::Exact`), everything else is its value.
fn key_predicate(field: GroupField, s: &str) -> AxisPredicate {
    if s == MISSING {
        return AxisPredicate::Absent;
    }
    if field.is_bucketed() {
        // Parse before `SolFilter` so the `u64` ceiling — which renders as exact
        // digits in BOTH precision modes — is recognised as the sentinel it is
        // instead of saturating into a bogus `i64::MAX` amount.
        if let Some(c) = ceiling_lamports(s) {
            return AxisPredicate::Ceiling(c);
        }
        return match SolFilter::parse(s) {
            Some(SolFilter::Exact(l)) => AxisPredicate::Lamports(l),
            Some(SolFilter::Range(lo, hi)) => AxisPredicate::LamportsRange { lo, hi },
            None => AxisPredicate::Text(s.to_string()),
        };
    }
    match field {
        GroupField::CuLimit | GroupField::CuPrice => {
            s.parse().map(AxisPredicate::Int).unwrap_or_else(|_| AxisPredicate::Text(s.to_string()))
        }
        GroupField::IsCashbackEnabled => {
            s.parse().map(AxisPredicate::Bool).unwrap_or_else(|_| AxisPredicate::Text(s.to_string()))
        }
        GroupField::IxLabels => {
            AxisPredicate::Labels(s.split(" | ").map(str::to_string).collect())
        }
        _ => AxisPredicate::Text(s.to_string()),
    }
}

/// `Some(lamports)` when a group-key label names an amount past the `i64` /
/// `BIGINT` axis domain — pump.fun's "fill at any price" ceiling.
fn ceiling_lamports(label: &str) -> Option<u64> {
    if label.contains('–') {
        return None;
    }
    let sol: f64 = label.trim().parse().ok()?;
    if sol < i64::MAX as f64 / LAMPORTS_PER_SOL_F64 {
        return None;
    }
    // Re-read the exact digits rather than round-tripping through the f64 that
    // just told us it is out of range (that's how `…709551615` became `…7096`).
    let (whole, frac) = match label.trim().split_once('.') {
        Some((w, f)) => (w, format!("{f:0<9}")),
        None => (label.trim(), "000000000".to_string()),
    };
    let w: u64 = whole.parse().ok()?;
    let f: u64 = frac.get(..9)?.parse().ok()?;
    w.checked_mul(1_000_000_000)?.checked_add(f)
}

/// The bucket width a `[lo, hi)` key implies, if it really is a bucket at that
/// width — i.e. `lo` is a multiple of `hi - lo`, which is what
/// `bucket_index(lo, w) * w == lo` asserts. `None` for any other range.
fn bucket_width_of(lo: i64, hi: i64) -> Option<f64> {
    if hi <= lo {
        return None;
    }
    let w = (hi - lo) as f64 / LAMPORTS_PER_SOL_F64;
    let lo_sol = lo as f64 / LAMPORTS_PER_SOL_F64;
    let edge = bucket_index(lo_sol, w) as f64 * w;
    if (edge - lo_sol).abs() <= w * BUCKET_EPS.max(1e-9) * 10.0 {
        Some(tidy_sol_decimal(w))
    } else {
        None
    }
}

/// Fold one axis's required width into the fingerprint-wide one, recording a
/// blocker when two axes disagree (the single `bucket_size_amount` can serve one).
fn reconcile(
    acc: &mut Option<Option<f64>>,
    want: Option<f64>,
    c: &Clause,
    blockers: &mut Vec<String>,
) {
    match acc {
        None => *acc = Some(want),
        Some(have) if same_width(*have, want) => {}
        Some(have) => blockers.push(blocked(
            c,
            &format!(
                "precision {} — but another axis needs {}, and a fingerprint has ONE bucket width",
                width_label(want),
                width_label(*have)
            ),
        )),
    }
}

fn same_width(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 1e-12,
        _ => false,
    }
}

fn width_label(w: Option<f64>) -> String {
    match w {
        None => "exact amounts".to_string(),
        Some(w) => format!("{w}-SOL buckets"),
    }
}

fn blocked(c: &Clause, why: &str) -> String {
    format!("{} = {} (from {}): {why}", c.field, c.display, c.origin.describe())
}

/// Local mirror of the engine's rounding conversion, kept here so a bucket edge
/// derived in SOL lands on the same lamports integer the axis stores.
fn sol_to_lamports_round(sol: f64) -> i64 {
    (sol * LAMPORTS_PER_SOL_F64).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run() -> GroupedSweepRun {
        GroupedSweepRun {
            id: Uuid::nil(),
            strategy_id: "generic".into(),
            source: "db".into(),
            method: "grid".into(),
            created_after: None,
            created_before: None,
            curve_only: false,
            grouping_spec: json!([]),
            axes_spec: json!({}),
            min_tokens: 1,
            token_count: 1,
            group_count: 1,
            combo_count: 1,
            corpus_hash: None,
            corpus_last_trade_at: None,
            created_at: chrono::Utc::now(),
            ix_labels_filter: None,
            field_filters: None,
            fingerprint_id: None,
            token_cap: None,
            max_combos: None,
            label: None,
            buy_amount_sol: Some(1.0),
            bucket_width_sol: None,
            fill_model: None,
            cost_model: None,
            volume_ix_patterns: None,
            scale_out: None,
            scale_out_top_k: None,
            status: "completed".into(),
            groups_done: 1,
        }
    }

    /// **The bug this module exists for.** A filter run pins the labels and one
    /// exact `max_sol_cost`, groups by nothing — and the promoted fingerprint used
    /// to carry the labels only, arming on every max-cost value there is.
    #[test]
    fn field_filters_reach_the_promoted_fingerprint() {
        let mut r = run();
        r.ix_labels_filter = Some(json!(["Pump.Fun: Create_v2", "Pump.Fun: Buy"]));
        r.field_filters = Some(json!({ "max_cost_lamports": ["0.324"] }));
        let sel = GroupSelection::resolve(&r, None, &json!({}));

        assert!(sel.promotable, "blockers: {:?}", sel.blockers);
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(fp.max_cost_lamports, Some(324_000_000), "the pinned amount must survive");
        assert_eq!(fp.ix_labels.as_deref(), Some(&["Pump.Fun: Create_v2".to_string(), "Pump.Fun: Buy".to_string()][..]));
        assert_eq!(fp.bucket_size_amount, None, "an exact pin is exact mode, not a bucket");
    }

    /// A bucketed group key round-trips to anchor + width, so the rule arms on the
    /// same bucket the group counted.
    #[test]
    fn bucketed_group_key_round_trips_to_anchor_plus_width() {
        let mut r = run();
        r.bucket_width_sol = Some(0.1);
        let sel = GroupSelection::resolve(&r, None, &json!({ "max_cost_lamports": "1.5–1.6" }));
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(fp.max_cost_lamports, Some(1_500_000_000));
        assert_eq!(fp.bucket_size_amount, Some(0.1));
    }

    /// Every clause a fingerprint cannot spell must BLOCK, never silently widen.
    /// One case per loss the old promote path had.
    #[test]
    fn unexpressible_clauses_block_instead_of_widening() {
        // `∅` — the axis is absent; dropping it matches tokens that have a value.
        let sel = GroupSelection::resolve(&run(), None, &json!({ "max_cost_lamports": "∅" }));
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("absent"), "{:?}", sel.blockers);

        // Axes the fingerprint has no column for.
        for key in ["token_program_id", "is_cashback_enabled"] {
            let gk = json!({ key: if key == "token_program_id" { json!("Tokenkeg") } else { json!("true") } });
            let sel = GroupSelection::resolve(&run(), None, &gk);
            assert!(!sel.promotable, "{key} must block");
        }

        // The `u64::MAX` "no cap" ceiling is a sentinel, not an amount.
        let sel = GroupSelection::resolve(
            &run(),
            None,
            &json!({ "max_cost_lamports": "18446744073.709551615" }),
        );
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("ceiling") || sel.blockers[0].contains("no cap"), "{:?}", sel.blockers);

        // A multi-value pin: a fingerprint axis holds one value.
        let mut r = run();
        r.field_filters = Some(json!({ "max_cost_lamports": ["0.1", "0.5"] }));
        let sel = GroupSelection::resolve(&r, None, &json!({}));
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("one value"), "{:?}", sel.blockers);

        // An exact pin on one axis + a bucketed group-by on another needs two
        // widths, and a fingerprint has one.
        let mut r = run();
        r.bucket_width_sol = Some(0.1);
        r.field_filters = Some(json!({ "max_cost_lamports": ["0.324"] }));
        let sel = GroupSelection::resolve(&r, None, &json!({ "initial_buy_sol": "1.5–1.6" }));
        assert!(!sel.promotable);
        assert!(sel.blockers.iter().any(|b| b.contains("ONE bucket width")), "{:?}", sel.blockers);
    }

    /// A selection with nothing expressible must not produce an all-`None`
    /// fingerprint (which the matcher refuses ⇒ a rule that never fires).
    #[test]
    fn empty_selection_is_not_promotable() {
        let sel = GroupSelection::resolve(&run(), None, &json!({}));
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("no fingerprint-expressible criterion"), "{:?}", sel.blockers);
    }

    /// A scoped run that ALSO grouped by an extra axis promoted the scope
    /// fingerprint verbatim — wider than the group. Both must come through.
    #[test]
    fn scoped_run_keeps_the_group_by_narrowing() {
        let mut fp = Fingerprint {
            id: Uuid::new_v4(),
            name: "scope".into(),
            cu_limit: Some(200_000),
            cu_price: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: None,
            ix_labels: Some(vec!["Pump.Fun: Create_v2".into()]),
            metric_config: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        fp.max_cost_lamports = Some(1_010_000);
        let mut r = run();
        r.fingerprint_id = Some(fp.id);
        let sel = GroupSelection::resolve(&r, Some(&fp), &json!({ "cu_price": "1000" }));
        let out = sel.materialize("t".into()).unwrap();
        assert_eq!(out.cu_limit, Some(200_000), "scope axis kept");
        assert_eq!(out.max_cost_lamports, Some(1_010_000), "scope axis kept");
        assert_eq!(out.cu_price, Some(1_000), "group-by narrowing kept");
    }

    /// A group-by axis is this group's own slice, so it wins over the wider
    /// clause that selected the corpus.
    #[test]
    fn group_by_overrides_the_corpus_filter_on_the_same_axis() {
        let mut r = run();
        r.field_filters = Some(json!({ "cu_limit": [200_000] }));
        let sel = GroupSelection::resolve(&r, None, &json!({ "cu_limit": "90000" }));
        let clause = sel.clauses.iter().find(|c| c.field == "cu_limit").unwrap();
        assert_eq!(clause.predicate, AxisPredicate::Int(90_000));
        assert_eq!(clause.origin, Origin::GroupBy);
    }
}
