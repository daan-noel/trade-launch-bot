//! **The one derivation of what a grouped-sweep group actually selected.**
//!
//! A group's token set is `corpus-window ∧ (scope-fingerprint | manual filters) ∧
//! group_key`. Every one of those clauses is persisted on the run row or the group
//! row, and no consumer may re-assemble them ad hoc: rebuilding a fingerprint from
//! `group_key` alone drops `field_filters`, and a second, differently-lossy
//! TypeScript rebuild badges the group card from the same partial input. Two
//! readers reconstructing one fact they were never handed both fail in the
//! **widening** direction — the promoted rule arms on a superset of the tokens the
//! numbers came from.
//!
//! This module is that fact, resolved once ([`GroupSelection::resolve`]) and
//! consumed by everything: promote materializes it into a fingerprint
//! ([`GroupSelection::materialize`]), the groups API serializes it for display.
//!
//! **A group key and a fingerprint axis are the same type.** Both carry an
//! [`AxisPredicate`], so promoting is a copy. What used to block a promote —
//! a bucketed key that had to be re-anchored to `value + width`, two axes needing
//! different widths, a `u64::MAX` ceiling no `BIGINT` axis could hold — is not
//! expressible as a failure any more, because none of those concepts exist.
//!
//! **Fail closed** on what remains. Three clause kinds genuinely have no
//! fingerprint expression: a multi-value filter (a predicate is one window), an
//! absent axis (a fingerprint spells "unset" as "unconstrained", which is the
//! opposite of what the group selected), and the two grouping-only fields the
//! matcher has no axis for. [`materialize`](GroupSelection::materialize) returns
//! those as blockers rather than dropping a clause and handing back a wider
//! fingerprint.

use std::fmt::Write as _;

use hunter_engine::fingerprint::{AxisId, AxisPredicate, AxisUnit, Criteria, SpanSet};
use hunter_engine::grouping::{
    normalize_labels, parse_filter, sol_label, GroupField, GroupKey, GroupValue,
};
use serde::Serialize;
use serde_json::Value;
use trading_core::models::Fingerprint;
use uuid::Uuid;

use crate::models::grouped_sweep::GroupedSweepRun;

/// Where a clause came from. Kept on every clause so an error message can name the
/// thing the user set ("the run's field filter", not "an axis").
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

/// What one axis is pinned to.
///
/// **Adjacently** tagged (`{"kind": …, "value": …}`), not internally tagged: an
/// internal tag can only be merged into a variant that serializes as a *map*, so a
/// scalar or sequence variant fails at runtime with "cannot serialize tagged
/// newtype variant …". That error surfaces nowhere near the type — it aborts
/// `to_value(&selection)` for the WHOLE selection, so the groups response ships
/// without `selection` at all and every card falls back to "no fingerprint".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ClauseValue {
    /// A predicate the fingerprint carries verbatim — the promotable case.
    Axis(AxisPredicate),
    /// Exact text (`token_program_id`). No fingerprint axis.
    Text(String),
    /// Exact boolean (`is_cashback_enabled`). No fingerprint axis.
    Flag(bool),
    /// Two or more alternatives, verbatim as the filter spelled them. One axis holds
    /// one predicate, so this never materializes.
    AnyOf(Vec<String>),
    /// The axis is **absent** on this group's tokens (the `∅` key). Distinct from
    /// "unconstrained": an unset fingerprint axis matches tokens that *have* a
    /// value, which is the opposite of what this group selected.
    Absent,
}

impl ClauseValue {
    /// Human-readable value, for the group card and for error messages.
    pub fn display(&self, unit: Option<AxisUnit>) -> String {
        let n = |v: &u128| match unit {
            Some(AxisUnit::Lamports) => sol_label(*v),
            _ => v.to_string(),
        };
        let window = |min: &Option<u128>, max: &Option<u128>| match (min, max) {
            (Some(a), Some(b)) if a == b => n(a),
            (Some(a), Some(b)) => format!("{}–{}", n(a), n(b)),
            (Some(a), None) => format!("≥{}", n(a)),
            (None, Some(b)) => format!("≤{}", n(b)),
            (None, None) => "any".to_string(),
        };
        match self {
            ClauseValue::Axis(AxisPredicate::Sequence { labels }) => labels.join(" | "),
            ClauseValue::Axis(AxisPredicate::Range { min, max }) => window(min, max),
            // A `!=` / `|` axis reads as the alternatives it accepts, in the same
            // window vocabulary — one span per alternative, so nothing about the
            // pinned set is hidden behind a summary.
            ClauseValue::Axis(AxisPredicate::Spans { spans }) => {
                spans.iter().map(|s| window(&s.min, &s.max)).collect::<Vec<_>>().join(" | ")
            }
            ClauseValue::Text(s) => s.clone(),
            ClauseValue::Flag(b) => b.to_string(),
            ClauseValue::AnyOf(vs) => format!("any of [{}]", vs.join(", ")),
            ClauseValue::Absent => "∅ (absent)".to_string(),
        }
    }
}

/// One axis of the selection: what it is pinned to, and who pinned it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clause {
    /// Serde tag of the axis (`"max_cost_lamports"`, …) — the key the group_key, the
    /// `field_filters` map and the frontend column labels all already use.
    pub field: &'static str,
    pub value: ClauseValue,
    pub origin: Origin,
    /// Pre-rendered `value.display()`, so the frontend never re-derives it.
    pub display: String,
}

/// Everything that selected one group's tokens, in one canonical vocabulary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GroupSelection {
    /// The saved fingerprint the run was scoped to, if any. Kept for provenance —
    /// the scope's axes are already expanded into `clauses`.
    pub scope_fingerprint_id: Option<Uuid>,
    /// Axis clauses, in [`GroupField`] declaration order (stable for display and for
    /// the `find_or_create` identity that follows from it).
    pub clauses: Vec<Clause>,
    /// Whether [`materialize`](Self::materialize) would succeed — precomputed so the
    /// group card can gray out Promote instead of failing on click.
    pub promotable: bool,
    /// Why not, when `promotable` is false. Empty otherwise.
    pub blockers: Vec<String>,
    /// The identity of the fingerprint this group promotes to — exactly what
    /// `FingerprintRepo::find_or_create` keys on. `None` when not promotable.
    ///
    /// Emitted so the frontend can tell whether that fingerprint already exists by
    /// *comparing* an identity the backend authored, instead of rebuilding one from
    /// the group key in TypeScript (a second derivation, and the reason a filtered
    /// run's card could never badge its own fingerprint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Value>,
}

/// Axis order for [`GroupSelection::clauses`] — labels first, then the registry's
/// own order, then the two grouping-only fields, so a selection reads like the rule
/// editor.
fn field_rank(tag: &str) -> usize {
    match tag {
        "ix_labels" => 0,
        "token_program_id" => 98,
        "is_cashback_enabled" => 99,
        other => AxisId::from_key(other).map(|a| a as usize + 1).unwrap_or(97),
    }
}

impl GroupSelection {
    /// Resolve `run` + this group's `group_key` into the canonical selection.
    ///
    /// `scope_fp` must be the run's `fingerprint_id` row when it has one (the caller
    /// owns the fetch); its axes are expanded into clauses so a scoped run and a
    /// filter run produce the *same* vocabulary downstream. A group-by axis wins over
    /// a scope/filter clause on the same field: it is this group's own slice, always
    /// at least as narrow as what selected the corpus.
    pub fn resolve(
        run: &GroupedSweepRun,
        scope_fp: Option<&Fingerprint>,
        group_key: &Value,
    ) -> Self {
        let mut acc: Vec<Clause> = Vec::new();

        // 1. The scope fingerprint's own axes. A straight copy — the fingerprint and
        //    the selection speak the same predicate vocabulary, so there is no
        //    precision to re-read and nothing that can be read differently here than
        //    the matcher reads it.
        if let Some(fp) = scope_fp {
            for (axis, pred) in fp.criteria.iter() {
                push(
                    &mut acc,
                    GroupField::from_axis(axis),
                    Some(ClauseValue::Axis(pred.clone())),
                    Origin::Scope,
                );
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
            .map(|labels| ClauseValue::Axis(AxisPredicate::Sequence { labels }));
        push(&mut acc, GroupField::IxLabels, labels, Origin::Filter);
        if let Some(map) = run.field_filters.as_ref().and_then(Value::as_object) {
            for (tag, vals) in map {
                let Some(field) = GroupField::from_tag(tag) else { continue };
                let Some(vals) = vals.as_array().filter(|v| !v.is_empty()) else { continue };
                push(&mut acc, field, Some(filter_value(field, vals)), Origin::Filter);
            }
        }

        // 3. This group's own key. Overrides the above: the group is a sub-slice of
        //    whatever selected the corpus.
        for (field, value) in GroupKey::from_json(group_key).0 {
            push(&mut acc, field, Some(key_value(&value)), Origin::GroupBy);
        }

        acc.sort_by_key(|c| field_rank(c.field));
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
                    "criteria": fp.criteria,
                    // `IDENTITY_WHERE` compares this too. A promoted group always names
                    // axes, so it is always `false` — but it has to be PRESENT, or the
                    // frontend compares an identity silently missing one of the columns
                    // it claims to carry, and a saved wildcard row answers to a group it
                    // does not match.
                    "wildcard": fp.wildcard,
                }));
            }
            Err(b) => sel.blockers = b,
        }
        sel
    }

    /// Build the fingerprint that matches **exactly** this selection, or return every
    /// clause that has no fingerprint expression.
    ///
    /// The returned fingerprint carries no `metric_config` — the caller owns that
    /// (the scope fingerprint's config / the run's volume-ix patterns).
    pub fn materialize(&self, name: String) -> Result<Fingerprint, Vec<String>> {
        let mut blockers: Vec<String> = Vec::new();
        let mut criteria = Criteria::new();

        for c in &self.clauses {
            let field = GroupField::from_tag(c.field);
            match (&c.value, field.and_then(GroupField::axis)) {
                // The promotable case, and now the common one: the predicate the
                // group selected on IS the predicate the fingerprint stores.
                (ClauseValue::Axis(pred), Some(axis)) => {
                    criteria.insert(axis, pred.clone());
                }
                (ClauseValue::Axis(_), None) => blockers.push(blocked(
                    c,
                    "a grouping-only field — the matcher has no axis for it",
                )),
                (ClauseValue::Absent, _) => blockers.push(blocked(
                    c,
                    "an absent axis — a fingerprint spells \"unset\" as \"unconstrained\", which \
                     would also match tokens that HAVE a value",
                )),
                (ClauseValue::AnyOf(_), _) => blockers.push(blocked(
                    c,
                    "several alternatives — one axis holds one predicate, and a range cannot \
                     express a disjunction",
                )),
                (ClauseValue::Text(_), _) => {
                    blockers.push(blocked(c, "the fingerprint has no token-program axis"))
                }
                (ClauseValue::Flag(_), _) => {
                    blockers.push(blocked(c, "the fingerprint has no cashback axis"))
                }
            }
        }

        let now = chrono::Utc::now();
        let fp = Fingerprint { name, criteria, ..Fingerprint::empty(Uuid::new_v4(), now) };

        // The matcher refuses a criterion-less fingerprint (it would otherwise arm on
        // everything), so an unexpressible-only selection must fail here rather than
        // create a row that silently matches nothing.
        if blockers.is_empty() {
            if let Err(e) = fp.validate() {
                blockers.push(if fp.criteria.is_empty() {
                    "this group has no fingerprint-expressible criterion — the rule would have \
                     no entry gate at all"
                        .to_string()
                } else {
                    e
                });
            }
        }
        if blockers.is_empty() {
            Ok(fp)
        } else {
            Err(blockers)
        }
    }

    /// Whether every clause came from the scope fingerprint — i.e. the group is the
    /// whole scope, with no group-by or filter narrowing on top. The promote path
    /// reuses the saved row itself in that case rather than minting a
    /// match-identical twin, which would be noise in the library.
    pub fn is_scope_only(&self) -> bool {
        self.scope_fingerprint_id.is_some()
            && !self.clauses.is_empty()
            && self.clauses.iter().all(|c| c.origin == Origin::Scope)
    }

    /// One-line summary for logs / the run header (`"ix_labels = … · max_cost_lamports = 0.324"`).
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
fn push(acc: &mut Vec<Clause>, field: GroupField, value: Option<ClauseValue>, origin: Origin) {
    let Some(value) = value else { return };
    let display = value.display(field.unit());
    let clause = Clause { field: field.as_str(), value, origin, display };
    match acc.iter_mut().find(|c| c.field == field.as_str()) {
        Some(slot) => *slot = clause,
        None => acc.push(clause),
    }
}

/// A `field_filters` entry → clause value. One entry parses through the shared
/// [`parse_filter`] (the same parser the corpus filter used, so the selection cannot
/// disagree with what was actually swept); two or more stay verbatim as
/// [`ClauseValue::AnyOf`].
fn filter_value(field: GroupField, vals: &[Value]) -> ClauseValue {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if vals.len() > 1 {
        return ClauseValue::AnyOf(vals.iter().map(text).collect());
    }
    let raw = text(&vals[0]);
    if let Some(unit) = field.unit() {
        return match parse_filter(&raw, unit) {
            Some(pred) => ClauseValue::Axis(pred),
            // Unparseable: the corpus filter dropped every token, so nothing reached
            // this group — but keep it visible rather than silently gone.
            None => ClauseValue::AnyOf(vec![raw]),
        };
    }
    match field {
        GroupField::IsCashbackEnabled => match vals[0].as_bool() {
            Some(b) => ClauseValue::Flag(b),
            None => ClauseValue::AnyOf(vec![raw]),
        },
        GroupField::IxLabels => {
            ClauseValue::Axis(AxisPredicate::Sequence {
                labels: raw.split(" | ").map(str::to_string).collect(),
            })
        }
        _ => ClauseValue::Text(raw),
    }
}

/// A `group_key` value → clause value. A direct read: the key already carries the
/// predicate, so unlike the retired form there is no label to parse and no `u64`
/// ceiling that has to be recognised before a float destroys its digits.
fn key_value(v: &GroupValue) -> ClauseValue {
    match v {
        GroupValue::Missing => ClauseValue::Absent,
        GroupValue::Text { value } => ClauseValue::Text(value.clone()),
        GroupValue::Flag { value } => ClauseValue::Flag(*value),
        GroupValue::Labels { labels } => {
            ClauseValue::Axis(AxisPredicate::Sequence { labels: labels.clone() })
        }
        GroupValue::Window { min, max } => {
            ClauseValue::Axis(AxisPredicate::Range { min: *min, max: *max })
        }
        GroupValue::Windows { spans } => {
            ClauseValue::Axis(SpanSet::from_spans(spans.iter().copied()).into_predicate())
        }
    }
}

fn blocked(c: &Clause, why: &str) -> String {
    format!("{} = {} (from {}): {why}", c.field, c.display, c.origin.describe())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOL: u128 = 1_000_000_000;

    fn run() -> GroupedSweepRun {
        GroupedSweepRun {
            id: Uuid::nil(),
            strategy_id: "generic".into(),
            source: "db".into(),
            method: "grid".into(),
            created_after: None,
            created_before: None,
            ..Default::default()
        }
    }

    fn fp_with(criteria: Criteria) -> Fingerprint {
        Fingerprint { criteria, ..Fingerprint::empty(Uuid::new_v4(), chrono::Utc::now()) }
    }

    fn window(min: Option<u128>, max: Option<u128>) -> Value {
        json!({
            "kind": "window",
            "min": min.map(|v| v.to_string()),
            "max": max.map(|v| v.to_string()),
        })
    }

    /// The headline property: a group key's window IS the fingerprint's range, so
    /// promoting copies it. Nothing is re-anchored, so the promoted rule arms on
    /// exactly the tokens the numbers came from.
    #[test]
    fn a_binned_group_promotes_to_the_same_window_it_selected() {
        let key = json!({ "max_cost_lamports": window(Some(SOL), Some(2 * SOL - 1)) });
        let sel = GroupSelection::resolve(&run(), None, &key);
        assert!(sel.promotable, "blockers: {:?}", sel.blockers);
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(
            fp.criteria.get(AxisId::MaxCostLamports),
            Some(&AxisPredicate::Range { min: Some(SOL), max: Some(2 * SOL - 1) })
        );
    }

    /// Two axes binned differently used to be unpromotable: one row-wide width could
    /// serve only one of them. Per-axis predicates make the conflict impossible.
    #[test]
    fn two_axes_at_different_granularities_now_promote_together() {
        let key = json!({
            "max_cost_lamports": window(Some(SOL), Some(2 * SOL - 1)),
            "init_buy_lamports": window(Some(1_515_000_000), Some(1_515_000_000)),
        });
        let sel = GroupSelection::resolve(&run(), None, &key);
        assert!(sel.promotable, "blockers: {:?}", sel.blockers);
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(fp.criteria.len(), 2);
        assert_eq!(
            fp.criteria.get(AxisId::InitBuyLamports).unwrap().as_exact(),
            Some(1_515_000_000)
        );
    }

    /// The `u64::MAX` "fill at any price" ceiling used to block a promote outright —
    /// no `BIGINT` axis could hold it. It is an ordinary bound now.
    #[test]
    fn a_no_cap_ceiling_promotes_like_any_other_amount() {
        let ceiling = u128::from(u64::MAX);
        let key = json!({ "max_cost_lamports": window(Some(ceiling), Some(ceiling)) });
        let sel = GroupSelection::resolve(&run(), None, &key);
        assert!(sel.promotable, "blockers: {:?}", sel.blockers);
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(fp.criteria.get(AxisId::MaxCostLamports).unwrap().as_exact(), Some(ceiling));
    }

    /// The two new axes ride the same path, with no promote-side change.
    #[test]
    fn the_derived_and_tallied_axes_promote_too() {
        let key = json!({
            "ix_count": window(Some(3), Some(5)),
            "prior_launches": window(Some(0), Some(0)),
        });
        let sel = GroupSelection::resolve(&run(), None, &key);
        assert!(sel.promotable, "blockers: {:?}", sel.blockers);
        let fp = sel.materialize("t".into()).unwrap();
        assert_eq!(
            fp.criteria.get(AxisId::IxCount),
            Some(&AxisPredicate::Range { min: Some(3), max: Some(5) })
        );
        assert_eq!(fp.criteria.get(AxisId::PriorLaunches).unwrap().as_exact(), Some(0));
    }

    /// An absent axis is NOT "unconstrained" — promoting it would match every token
    /// that has a value, the opposite of what the group selected.
    #[test]
    fn an_absent_axis_blocks_the_promote() {
        let key = json!({ "max_cost_lamports": { "kind": "missing" } });
        let sel = GroupSelection::resolve(&run(), None, &key);
        assert!(!sel.promotable);
        assert_eq!(sel.blockers.len(), 1, "{:?}", sel.blockers);
        assert!(sel.blockers[0].contains("absent"), "{:?}", sel.blockers);
    }

    /// A grouping-only field has no matcher axis, so a group keyed on it cannot
    /// become a rule — and must say so rather than promote a wider fingerprint.
    #[test]
    fn a_grouping_only_field_blocks_the_promote() {
        for key in [
            json!({ "token_program_id": { "kind": "text", "value": "Tokenkeg" } }),
            json!({ "is_cashback_enabled": { "kind": "flag", "value": true } }),
        ] {
            let sel = GroupSelection::resolve(&run(), None, &key);
            assert!(!sel.promotable, "{key} promoted");
        }
    }

    /// A multi-value filter is a disjunction; one range cannot express it.
    #[test]
    fn a_multi_value_filter_blocks_the_promote() {
        let mut r = run();
        r.field_filters = Some(json!({ "cu_limit": ["200000", "300000"] }));
        let sel = GroupSelection::resolve(&r, None, &json!({}));
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("alternatives"), "{:?}", sel.blockers);
    }

    /// A selection with nothing expressible must FAIL, never produce a
    /// criterion-less row: the matcher reads that as "match nothing", which silently
    /// kills the promoted rule.
    #[test]
    fn an_empty_selection_is_not_a_fingerprint() {
        let sel = GroupSelection::resolve(&run(), None, &json!({}));
        assert!(!sel.promotable);
        assert!(sel.blockers[0].contains("no entry gate"), "{:?}", sel.blockers);
    }

    /// A group-by axis is this group's own slice, so it overrides the corpus-level
    /// clause on the same field — and the resulting clause says so.
    #[test]
    fn a_group_by_axis_overrides_the_scope_on_the_same_field() {
        let scope = fp_with(
            Criteria::new()
                .with(AxisId::MaxCostLamports, AxisPredicate::range(Some(SOL), Some(9 * SOL))),
        );
        let mut r = run();
        r.fingerprint_id = Some(scope.id);
        let key = json!({ "max_cost_lamports": window(Some(2 * SOL), Some(3 * SOL - 1)) });
        let sel = GroupSelection::resolve(&r, Some(&scope), &key);
        let c = sel.clauses.iter().find(|c| c.field == "max_cost_lamports").unwrap();
        assert_eq!(c.origin, Origin::GroupBy);
        assert_eq!(
            c.value,
            ClauseValue::Axis(AxisPredicate::Range { min: Some(2 * SOL), max: Some(3 * SOL - 1) })
        );
        assert!(!sel.is_scope_only());
    }

    /// A scope with no narrowing on top reuses the saved row rather than minting a
    /// match-identical twin.
    #[test]
    fn a_scope_only_group_is_recognised() {
        let scope = fp_with(Criteria::new().with(AxisId::CuLimit, AxisPredicate::exact(200_000)));
        let mut r = run();
        r.fingerprint_id = Some(scope.id);
        let sel = GroupSelection::resolve(&r, Some(&scope), &json!({}));
        assert!(sel.is_scope_only());
        assert!(sel.promotable);
    }

    /// The identity the frontend compares must carry every column `IDENTITY_WHERE`
    /// keys on — a missing one lets a saved wildcard row badge a group it does not
    /// match.
    #[test]
    fn the_emitted_identity_carries_every_identity_column() {
        let key = json!({ "cu_limit": window(Some(200_000), Some(200_000)) });
        let sel = GroupSelection::resolve(&run(), None, &key);
        let id = sel.identity.expect("promotable");
        assert!(id.get("criteria").is_some() && id.get("wildcard").is_some(), "{id}");
        assert_eq!(id["wildcard"], json!(false));
    }

    /// Display reads in the axis's own unit — SOL for a lamports axis, the integer
    /// for a tally — so a card never shows 1500000000 where a human types 1.5.
    #[test]
    fn clause_display_reads_in_the_axis_unit() {
        let key = json!({
            "max_cost_lamports": window(Some(1_515_000_000), Some(1_515_000_000)),
            "ix_count": window(Some(3), Some(5)),
        });
        let sel = GroupSelection::resolve(&run(), None, &key);
        let by = |f: &str| sel.clauses.iter().find(|c| c.field == f).unwrap().display.clone();
        assert_eq!(by("max_cost_lamports"), "1.515");
        assert_eq!(by("ix_count"), "3–5");
    }

    /// The selection has to survive `serde_json::to_value` — an internally tagged
    /// enum aborts the whole serialization and the card silently loses `selection`.
    #[test]
    fn a_selection_serializes_with_every_clause_kind_present() {
        let mut r = run();
        r.field_filters = Some(json!({ "cu_price": ["1", "2"] }));
        let key = json!({
            "token_program_id": { "kind": "text", "value": "Tokenkeg" },
            "is_cashback_enabled": { "kind": "flag", "value": true },
            "max_cost_lamports": { "kind": "missing" },
            "cu_limit": window(Some(1), Some(1)),
            "ix_labels": { "kind": "labels", "labels": ["A", "B"] },
        });
        let sel = GroupSelection::resolve(&r, None, &key);
        let v = serde_json::to_value(&sel).expect("selection must serialize");
        // Five from the key, plus the `cu_price` multi-value run filter.
        assert_eq!(v["clauses"].as_array().unwrap().len(), 6);
    }
}
