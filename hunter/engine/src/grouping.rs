//! Strategy-agnostic token grouping — partition a corpus by observed creation axes.
//!
//! A sweep splits its corpus by a **compound group key**: the chosen fields' values
//! under a per-field [`PartitionSpec`]. Each surviving group is swept independently,
//! so the UI can answer "for tokens with *this* creation shape, which param combo is
//! best?".
//!
//! **A group key carries predicates, not rendered labels.** A continuous axis is
//! partitioned into explicit `[min, max]` windows, and the window IS an
//! [`AxisPredicate`] — the same type a fingerprint stores. Promoting a group to a
//! fingerprint is therefore a copy, not a reconstruction: there is no `"lo–hi"`
//! string to parse back, no implicit `floor(v/width)` lattice for a second
//! implementation to reproduce, and no boundary epsilon to keep in lockstep with
//! SQL. Display strings are rendered *from* the key ([`GroupValue::render`]) and are
//! never read back.
//!
//! This module is deliberately strategy-blind: it only reads [`TokenFingerprint`]
//! and never touches the `Strategy`/`ParamSpace` surface.
//!
//! Creator wallet is **deliberately not** a grouping dimension: on pump.fun creators
//! rotate wallets constantly, so a creator key is un-trackable across tokens and
//! only ever yields singleton groups.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fingerprint::{AxisId, AxisKind, AxisPredicate, AxisUnit, Span, SpanSet};

// `TokenFingerprint` and its decode seams live with the matcher (they are the
// matcher's input); re-exported here because every grouping caller reads them.
pub use crate::fingerprint::{
    extract_lamports, lamports_to_sol, normalize_labels, sol_to_lamports, TokenFingerprint,
};

/// Lamports per SOL as `f64` — the display-side divisor. Identity is integer, so
/// this is only ever used to render or to parse a typed amount.
pub const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;

/// Sentinel rendered for a token that does not carry a grouped field, so those
/// tokens form their own group instead of colliding with `0`/`""`.
pub const MISSING: &str = "∅";

// ─────────────────────────────────────────────────────────────────────────────
// Fields
// ─────────────────────────────────────────────────────────────────────────────

/// One selectable grouping field. Serde snake_case tags match the `group_by` array
/// the frontend sends and the keys stored in a [`GroupKey`]'s JSON.
///
/// Most fields *are* fingerprint axes ([`Self::axis`]) and so can be promoted into a
/// rule. Two are grouping-only — a token's program id and its cashback flag identify
/// no creation shape the matcher knows, so a group keyed on them cannot become a
/// fingerprint, and [`Self::axis`] returning `None` is how every promote path learns
/// that without a second list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupField {
    TokenProgramId,
    IsCashbackEnabled,
    CuLimit,
    CuPrice,
    /// Aliased: stored group keys written while this field named a SOL `f64` still
    /// parse. It is integer lamports now, per the unit-naming rule.
    #[serde(alias = "initial_buy_sol")]
    InitBuyLamports,
    MaxCostLamports,
    SpendableLamportsIn,
    #[serde(alias = "first_slot_buy_sol")]
    FirstSlotBuyLamports,
    #[serde(alias = "first_slot_sell_sol")]
    FirstSlotSellLamports,
    IxLabels,
    IxCount,
    PriorLaunches,
    CreateAta,
}

impl GroupField {
    /// Every field, for exhaustive iteration in guards and request validation.
    pub const ALL: [GroupField; 13] = [
        GroupField::TokenProgramId,
        GroupField::IsCashbackEnabled,
        GroupField::CuLimit,
        GroupField::CuPrice,
        GroupField::InitBuyLamports,
        GroupField::MaxCostLamports,
        GroupField::SpendableLamportsIn,
        GroupField::FirstSlotBuyLamports,
        GroupField::FirstSlotSellLamports,
        GroupField::IxLabels,
        GroupField::IxCount,
        GroupField::PriorLaunches,
        GroupField::CreateAta,
    ];

    /// The fingerprint axis this field groups on, or `None` for a grouping-only
    /// field that no rule can name.
    pub fn axis(self) -> Option<AxisId> {
        Some(match self {
            GroupField::TokenProgramId | GroupField::IsCashbackEnabled => return None,
            GroupField::CuLimit => AxisId::CuLimit,
            GroupField::CuPrice => AxisId::CuPrice,
            GroupField::InitBuyLamports => AxisId::InitBuyLamports,
            GroupField::MaxCostLamports => AxisId::MaxCostLamports,
            GroupField::SpendableLamportsIn => AxisId::SpendableLamportsIn,
            GroupField::FirstSlotBuyLamports => AxisId::FirstSlotBuyLamports,
            GroupField::FirstSlotSellLamports => AxisId::FirstSlotSellLamports,
            GroupField::IxLabels => AxisId::IxLabels,
            GroupField::IxCount => AxisId::IxCount,
            GroupField::PriorLaunches => AxisId::PriorLaunches,
            GroupField::CreateAta => AxisId::CreateAta,
        })
    }

    /// The field for an axis — the inverse of [`Self::axis`].
    pub fn from_axis(axis: AxisId) -> GroupField {
        GroupField::ALL
            .into_iter()
            .find(|f| f.axis() == Some(axis))
            .expect("every axis is a group field")
    }

    /// Stable key used in the [`GroupKey`] JSON object (matches the serde tag).
    pub fn as_str(self) -> &'static str {
        match self {
            GroupField::TokenProgramId => "token_program_id",
            GroupField::IsCashbackEnabled => "is_cashback_enabled",
            GroupField::IxLabels => "ix_labels",
            other => other.axis().expect("non-axis fields handled above").key(),
        }
    }

    /// Parse a serde snake_case tag back to a field, accepting the retired
    /// `*_sol` spellings so a stored run's `group_by` still resolves.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let t = tag.trim();
        Some(match t {
            "token_program_id" => GroupField::TokenProgramId,
            "is_cashback_enabled" => GroupField::IsCashbackEnabled,
            "initial_buy_sol" => GroupField::InitBuyLamports,
            "first_slot_buy_sol" => GroupField::FirstSlotBuyLamports,
            "first_slot_sell_sol" => GroupField::FirstSlotSellLamports,
            _ => GroupField::ALL.into_iter().find(|f| f.as_str() == t)?,
        })
    }

    /// Human label for a chip or a column header.
    pub fn label(self) -> &'static str {
        match self {
            GroupField::TokenProgramId => "Token program",
            GroupField::IsCashbackEnabled => "Cashback",
            other => other.axis().expect("non-axis fields handled above").def().label,
        }
    }

    /// The unit a typed bound is read in, for the fields that carry numbers.
    pub fn unit(self) -> Option<AxisUnit> {
        let axis = self.axis()?;
        match axis.def().kind {
            AxisKind::Numeric => Some(axis.def().unit),
            AxisKind::Sequence => None,
        }
    }

    /// Whether this field's value is a number, i.e. whether a [`PartitionSpec`]
    /// other than [`PartitionSpec::Distinct`] means anything on it.
    pub fn is_numeric(self) -> bool {
        self.unit().is_some()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Partitioning
// ─────────────────────────────────────────────────────────────────────────────

/// How one field's values are collapsed into group keys.
///
/// **There is no width.** A width defines an infinite implicit lattice that every
/// consumer has to re-derive identically (and a `0` in it is a division by zero);
/// explicit edges are a finite list that travels with the run and means the same
/// thing to everyone who reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartitionSpec {
    /// One group per distinct value — the only mode for a discrete field.
    Distinct,
    /// Bin into windows at explicit ascending edges. Edge `i` opens the window
    /// `[edges[i], edges[i+1] - 1]`; the last is open-topped, and a value below
    /// `edges[0]` falls in an open-bottomed window. So the edges tile the whole
    /// domain — no value is dropped for sitting outside them.
    Ranges { edges: Vec<u128> },
}

impl PartitionSpec {
    /// Windows covering the observed `values`, split at `n` roughly equal-count
    /// quantiles. Empty input, `n < 2`, or values too repetitive to split yields
    /// [`Self::Distinct`] — a partition with one window is not a partition, and
    /// saying so beats returning edges that group everything together.
    pub fn quantiles(values: &[u128], n: usize) -> PartitionSpec {
        if n < 2 || values.len() < n {
            return PartitionSpec::Distinct;
        }
        let mut sorted: Vec<u128> = values.to_vec();
        sorted.sort_unstable();
        let floor = sorted[0];
        let mut edges: Vec<u128> = Vec::with_capacity(n - 1);
        for i in 1..n {
            let at = i * sorted.len() / n;
            let edge = sorted[at.min(sorted.len() - 1)];
            // Two guards, both about edges that split nothing. An edge at or below
            // the smallest observed value opens a window no token can fall below, and
            // a repeated edge opens an empty one; dropping both keeps the list
            // strictly ascending, which is what `window_for` binary-searches on, and
            // keeps every window non-empty, so a group count means what it says.
            if edge > floor && edges.last() != Some(&edge) {
                edges.push(edge);
            }
        }
        if edges.is_empty() {
            PartitionSpec::Distinct
        } else {
            PartitionSpec::Ranges { edges }
        }
    }

    /// The window containing `v`. A partition tiles the domain, so every value
    /// lands in exactly ONE inclusive [`Span`] — a partition can never produce the
    /// multi-span shape `!=`/`|` do, and saying so in the type keeps the group-key
    /// builder free of an arm that cannot happen.
    fn window_for(&self, v: u128) -> Span {
        match self {
            PartitionSpec::Distinct => Span::exact(v),
            PartitionSpec::Ranges { edges } => {
                // `partition_point` gives how many edges are <= v: that index is the
                // window, with `edges[i-1]` its floor and `edges[i]` the next floor.
                let i = edges.partition_point(|e| *e <= v);
                let min = if i == 0 { None } else { Some(edges[i - 1]) };
                let max = edges.get(i).map(|e| e - 1);
                Span::new(min, max)
            }
        }
    }
}

/// Which fields a run groups by, and how each is partitioned. An empty plan is the
/// single "ALL" group.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupPlan(pub Vec<(GroupField, PartitionSpec)>);

impl GroupPlan {
    /// One group per distinct value on every field.
    pub fn distinct(fields: &[GroupField]) -> GroupPlan {
        GroupPlan(fields.iter().map(|f| (*f, PartitionSpec::Distinct)).collect())
    }

    /// The same spec on every numeric field; discrete fields stay
    /// [`PartitionSpec::Distinct`] regardless, since binning a program id is
    /// meaningless.
    pub fn uniform(fields: &[GroupField], spec: PartitionSpec) -> GroupPlan {
        GroupPlan(
            fields
                .iter()
                .map(|f| {
                    let s = if f.is_numeric() { spec.clone() } else { PartitionSpec::Distinct };
                    (*f, s)
                })
                .collect(),
        )
    }

    pub fn fields(&self) -> impl Iterator<Item = GroupField> + '_ {
        self.0.iter().map(|(f, _)| *f)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Keys
// ─────────────────────────────────────────────────────────────────────────────

/// One field's value in a group key.
///
/// Numeric fields carry a predicate, so a key is directly promotable to a
/// fingerprint axis; the display string is derived from it and never parsed back.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupValue {
    /// The token does not carry this field. Distinct from any real value — "absent"
    /// and "zero" are different facts, and a group that merged them would be a lie
    /// about what its tokens have in common.
    Missing,
    /// A discrete string (program id).
    Text { value: String },
    /// A boolean flag.
    Flag { value: bool },
    /// An exact ordered label sequence.
    Labels { labels: Vec<String> },
    /// Two or more disjoint numeric windows — what a `!=` or `|` fingerprint axis
    /// asserts. A partition never produces one (it tiles the domain into single
    /// windows); this exists so a SAVED fingerprint can be shown as a group key
    /// without dropping the axis, which would read as "unconstrained".
    Windows { spans: Vec<Span> },
    /// An inclusive numeric window. `min == max` is one exact value.
    Window {
        #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::fingerprint::axis::u128_wire")]
        min: Option<u128>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::fingerprint::axis::u128_wire")]
        max: Option<u128>,
    },
}

impl GroupValue {
    /// The fingerprint predicate this value asserts, or `None` when the field names
    /// nothing a rule can match on (`Missing`, or a grouping-only field).
    pub fn to_predicate(&self) -> Option<AxisPredicate> {
        match self {
            GroupValue::Window { min, max } => Some(AxisPredicate::Range { min: *min, max: *max }),
            GroupValue::Windows { spans } => {
                Some(SpanSet::from_spans(spans.iter().copied()).into_predicate())
            }
            GroupValue::Labels { labels } => {
                Some(AxisPredicate::Sequence { labels: labels.clone() })
            }
            GroupValue::Missing | GroupValue::Text { .. } | GroupValue::Flag { .. } => None,
        }
    }

    /// Display text for a chip. **Rendering only** — nothing parses this back, which
    /// is why it is free to be as readable as it likes.
    pub fn render(&self, unit: Option<AxisUnit>) -> String {
        let n = |v: u128| match unit {
            Some(AxisUnit::Lamports) => sol_label(v),
            _ => v.to_string(),
        };
        match self {
            GroupValue::Missing => MISSING.to_string(),
            GroupValue::Text { value } => value.clone(),
            GroupValue::Flag { value } => value.to_string(),
            GroupValue::Labels { labels } => labels.join(" | "),
            GroupValue::Window { min, max } => render_window(*min, *max, &n),
            GroupValue::Windows { spans } => {
                spans.iter().map(|s| render_window(s.min, s.max, &n)).collect::<Vec<_>>().join(" | ")
            }
        }
    }
}

/// One window, in the caller's display unit. Shared by the single- and multi-window
/// values so both read the same way — display only; nothing parses this back.
fn render_window(min: Option<u128>, max: Option<u128>, n: &dyn Fn(u128) -> String) -> String {
    match (min, max) {
        (Some(a), Some(b)) if a == b => n(a),
        (Some(a), Some(b)) => format!("{}–{}", n(a), n(b)),
        (Some(a), None) => format!("≥{}", n(a)),
        (None, Some(b)) => format!("≤{}", n(b)),
        (None, None) => "any".to_string(),
    }
}

/// A compound group key: each grouped field's value, in selection order. An empty
/// `Vec` is the single "ALL" group (no grouping selected ⇒ one global sweep).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupKey(pub Vec<(GroupField, GroupValue)>);

impl GroupKey {
    /// `{"cu_limit": {"kind":"window","min":"200000","max":"200000"}}` — stored on
    /// the group row. Structured, so the promote path reads predicates rather than
    /// re-parsing prose.
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.0.len());
        for (f, v) in &self.0 {
            map.insert(f.as_str().to_string(), serde_json::to_value(v).unwrap_or(Value::Null));
        }
        Value::Object(map)
    }

    /// Parse a stored key back. Unknown fields are skipped — a run stored before a
    /// field existed still resolves the fields it does carry.
    pub fn from_json(v: &Value) -> GroupKey {
        let Some(obj) = v.as_object() else { return GroupKey(Vec::new()) };
        GroupKey(
            obj.iter()
                .filter_map(|(k, val)| {
                    let f = GroupField::from_tag(k)?;
                    let gv = serde_json::from_value::<GroupValue>(val.clone()).ok()?;
                    Some((f, gv))
                })
                .collect(),
        )
    }

    /// `{"cu_limit": "200000"}` — the display form, for a chip row.
    pub fn render(&self) -> Vec<(GroupField, String)> {
        self.0.iter().map(|(f, v)| (*f, v.render(f.unit()))).collect()
    }
}

/// The group key one token falls in under `plan`.
pub fn group_key(tf: &TokenFingerprint, plan: &GroupPlan) -> GroupKey {
    GroupKey(plan.0.iter().map(|(f, spec)| (*f, field_value(tf, *f, spec))).collect())
}

/// One field's value for one token.
fn field_value(tf: &TokenFingerprint, f: GroupField, spec: &PartitionSpec) -> GroupValue {
    match f {
        GroupField::TokenProgramId => match &tf.token_program_id {
            Some(v) => GroupValue::Text { value: v.clone() },
            None => GroupValue::Missing,
        },
        GroupField::IsCashbackEnabled => GroupValue::Flag { value: tf.is_cashback_enabled },
        GroupField::IxLabels => {
            // Empty ≡ absent, decided by the same SSOT the matcher uses: this key is
            // read back into a fingerprint axis, so the token side and the
            // fingerprint side have to agree about what "unset" is.
            match crate::fingerprint::configured_labels(Some(&tf.ix_labels)) {
                Some(l) => GroupValue::Labels { labels: l.to_vec() },
                None => GroupValue::Missing,
            }
        }
        other => {
            let axis = other.axis().expect("non-axis fields handled above");
            match axis.read_num(tf) {
                None => GroupValue::Missing,
                Some(v) => {
                    let w = spec.window_for(v);
                    GroupValue::Window { min: w.min, max: w.max }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Typed filters
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a typed value filter into the **same predicate a fingerprint stores**, so
/// the filter box, the group key and the match all speak one vocabulary.
///
/// A thin alias for [`crate::fingerprint::grammar::parse_predicate`] — there is one
/// text ⇄ predicate translation, and a filter box that had its own would let a chip
/// mean two things depending on which field it was pasted into. The forms, the
/// `..`-vs-`-` distinction and the strictness all live in that module's docs.
///
/// `None` on anything malformed — a dropped filter reads as "no filter", which
/// silently widens a query instead of failing it.
pub fn parse_filter(text: &str, unit: AxisUnit) -> Option<AxisPredicate> {
    crate::fingerprint::grammar::parse_predicate(text, unit)
}

/// Render lamports as human SOL, exactly. Integer arithmetic: `lamports as f64 / 1e9`
/// is lossless only below 2^53, and a `u64::MAX` ceiling is real data whose low
/// digits a float round-trip silently drops — mapping distinct amounts onto one
/// label.
pub fn sol_label(lamports: u128) -> String {
    let whole = lamports / 1_000_000_000;
    let frac = lamports % 1_000_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    format!("{whole}.{frac:09}").trim_end_matches('0').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOL: u128 = 1_000_000_000;

    fn tf() -> TokenFingerprint {
        TokenFingerprint {
            token_program_id: Some("Tokenkeg".into()),
            is_cashback_enabled: true,
            cu_limit: Some(200_000),
            cu_price: None,
            init_buy_lamports: Some(1_500_000_000),
            max_cost_lamports: Some(1_000_000_000),
            spendable_lamports_in: None,
            first_slot_buy_lamports: Some(2_250_000_000),
            first_slot_sell_lamports: None,
            ix_labels: vec!["Pump.Fun: Create".into(), "System: Transfer".into()],
            prior_launches: Some(3),
        }
    }

    #[test]
    fn every_field_round_trips_through_its_tag_and_names_an_axis_or_is_declared_grouping_only() {
        for f in GroupField::ALL {
            assert_eq!(GroupField::from_tag(f.as_str()), Some(f), "{f:?}");
            if let Some(axis) = f.axis() {
                assert_eq!(GroupField::from_axis(axis), f);
                assert_eq!(f.as_str(), axis.key(), "{f:?}: tag must equal the axis key");
            }
        }
        // The retired `*_sol` spellings still resolve, so a stored run's group_by
        // does not become unreadable.
        assert_eq!(GroupField::from_tag("initial_buy_sol"), Some(GroupField::InitBuyLamports));
        assert_eq!(GroupField::from_tag("first_slot_buy_sol"), Some(GroupField::FirstSlotBuyLamports));
        assert_eq!(GroupField::from_tag("first_slot_sell_sol"), Some(GroupField::FirstSlotSellLamports));
        assert_eq!(GroupField::from_tag("nope"), None);
        // Exactly two fields are grouping-only.
        assert_eq!(GroupField::ALL.iter().filter(|f| f.axis().is_none()).count(), 2);
    }

    /// Distinct mode pins the value; a range plan bins it. Both produce a predicate
    /// the fingerprint matcher can consume unchanged.
    #[test]
    fn a_group_key_carries_predicates_a_fingerprint_can_use_verbatim() {
        let distinct = group_key(&tf(), &GroupPlan::distinct(&[GroupField::InitBuyLamports]));
        let pred = distinct.0[0].1.to_predicate().unwrap();
        assert_eq!(pred.as_exact(), Some(1_500_000_000));
        assert!(pred.matches_num(1_500_000_000) && !pred.matches_num(1_500_000_001));

        let plan = GroupPlan(vec![(
            GroupField::InitBuyLamports,
            PartitionSpec::Ranges { edges: vec![SOL, 2 * SOL] },
        )]);
        let binned = group_key(&tf(), &plan);
        let pred = binned.0[0].1.to_predicate().unwrap();
        assert_eq!(pred, AxisPredicate::Range { min: Some(SOL), max: Some(2 * SOL - 1) });
        assert!(pred.matches_num(SOL) && pred.matches_num(2 * SOL - 1));
        assert!(!pred.matches_num(2 * SOL), "the next edge opens the next window");
    }

    /// Edges tile the whole domain: nothing below the first edge or above the last
    /// is dropped, and adjacent windows never overlap.
    #[test]
    fn range_edges_tile_the_domain_without_gaps_or_overlap() {
        let spec = PartitionSpec::Ranges { edges: vec![10, 20, 30] };
        let win = |v: u128| spec.window_for(v);
        assert_eq!(win(0), Span::new(None, Some(9)));
        assert_eq!(win(9), Span::new(None, Some(9)));
        assert_eq!(win(10), Span::new(Some(10), Some(19)));
        assert_eq!(win(19), Span::new(Some(10), Some(19)));
        assert_eq!(win(20), Span::new(Some(20), Some(29)));
        assert_eq!(win(30), Span::new(Some(30), None));
        assert_eq!(win(u128::MAX), Span::new(Some(30), None));
        // Every value lands in exactly one window, and that window contains it.
        for v in [0u128, 9, 10, 19, 20, 29, 30, 1_000] {
            assert!(win(v).contains(v), "{v} fell outside its own window");
        }
    }

    #[test]
    fn quantiles_split_by_count_and_refuse_to_pretend_when_they_cannot() {
        let values: Vec<u128> = (0..100).collect();
        let PartitionSpec::Ranges { edges } = PartitionSpec::quantiles(&values, 4) else {
            panic!("100 distinct values must split into 4");
        };
        assert_eq!(edges, vec![25, 50, 75]);
        // Too few values, or a degenerate ask, is Distinct — never a single window
        // dressed up as a partition.
        assert_eq!(PartitionSpec::quantiles(&values, 1), PartitionSpec::Distinct);
        assert_eq!(PartitionSpec::quantiles(&[1, 2], 4), PartitionSpec::Distinct);
        // All-identical values cannot be split, and say so.
        assert_eq!(PartitionSpec::quantiles(&[7; 40], 4), PartitionSpec::Distinct);
    }

    #[test]
    fn missing_stays_its_own_group_and_never_collides_with_zero() {
        let plan = GroupPlan::distinct(&[GroupField::CuPrice]);
        let missing = group_key(&tf(), &plan);
        let zero = group_key(&TokenFingerprint { cu_price: Some(0), ..tf() }, &plan);
        assert_ne!(missing, zero);
        assert_eq!(missing.0[0].1, GroupValue::Missing);
        assert_eq!(missing.0[0].1.render(None), MISSING);
        // Missing promotes to nothing — it asserts no predicate a rule could arm on.
        assert_eq!(missing.0[0].1.to_predicate(), None);
    }

    #[test]
    fn a_key_round_trips_through_stored_json() {
        let plan = GroupPlan(vec![
            (GroupField::MaxCostLamports, PartitionSpec::Ranges { edges: vec![SOL, 2 * SOL] }),
            (GroupField::CuLimit, PartitionSpec::Distinct),
            (GroupField::IxLabels, PartitionSpec::Distinct),
            (GroupField::TokenProgramId, PartitionSpec::Distinct),
        ]);
        let key = group_key(&tf(), &plan);
        let json = key.to_json();
        assert_eq!(
            json["max_cost_lamports"],
            serde_json::json!({ "kind": "window", "min": "1000000000", "max": "1999999999" })
        );
        let back = GroupKey::from_json(&json);
        // Field order comes from the JSON object, so compare as sets of pairs.
        let mut a = key.0.clone();
        let mut b = back.0.clone();
        a.sort_by_key(|(f, _)| f.as_str());
        b.sort_by_key(|(f, _)| f.as_str());
        assert_eq!(a, b);
    }

    /// A ceiling must survive the key intact — it is the value a float round-trip
    /// destroys, and a group key is stored as JSON.
    #[test]
    fn a_ceiling_survives_the_key_and_its_label() {
        let t = TokenFingerprint { max_cost_lamports: Some(u64::MAX), ..tf() };
        let key = group_key(&t, &GroupPlan::distinct(&[GroupField::MaxCostLamports]));
        assert_eq!(key.0[0].1.to_predicate().unwrap().as_exact(), Some(u128::from(u64::MAX)));
        let back = GroupKey::from_json(&key.to_json());
        assert_eq!(back.0[0].1, key.0[0].1);
        assert_eq!(sol_label(u128::from(u64::MAX)), "18446744073.709551615");
    }

    #[test]
    fn sol_labels_read_as_typed_amounts() {
        assert_eq!(sol_label(1_515_000_000), "1.515");
        assert_eq!(sol_label(1_000_000_000), "1");
        assert_eq!(sol_label(100_000_000_000), "100");
        assert_eq!(sol_label(0), "0");
        assert_eq!(sol_label(1), "0.000000001");
    }

    /// A chip's own text, pasted into the filter box, selects exactly that chip's
    /// tokens — the property that makes the range syntax discoverable.
    #[test]
    fn a_chip_label_round_trips_as_a_filter_over_the_same_window() {
        let spec = PartitionSpec::Ranges { edges: vec![SOL, 2 * SOL, 5 * SOL] };
        for v in [0u128, SOL, SOL + 1, 2 * SOL, 4 * SOL, 9 * SOL] {
            let span = spec.window_for(v);
            let window = AxisPredicate::Range { min: span.min, max: span.max };
            let text = GroupValue::Window { min: span.min, max: span.max }
                .render(Some(AxisUnit::Lamports));
            let Some(parsed) = parse_filter(&text, AxisUnit::Lamports) else {
                continue; // an open-ended chip renders "≥…", covered below
            };
            for other in [0u128, SOL, SOL + 1, 2 * SOL, 4 * SOL, 9 * SOL] {
                assert_eq!(
                    parsed.matches_num(other),
                    window.matches_num(other),
                    "chip {text:?} disagrees with its own window on {other}"
                );
            }
        }
    }

    #[test]
    fn filters_parse_every_form_and_reject_junk() {
        let l = AxisUnit::Lamports;
        assert_eq!(parse_filter("1.515", l).unwrap().as_exact(), Some(1_515_000_000));
        assert_eq!(parse_filter(" 1.515 ", l).unwrap().as_exact(), Some(1_515_000_000));
        // Half-open, so a chip's upper edge belongs to the next window.
        let r = parse_filter("1.5–1.6", l).unwrap();
        assert_eq!(parse_filter("1.5-1.6", l), Some(r.clone()));
        assert!(r.matches_num(1_500_000_000) && r.matches_num(1_599_999_999));
        assert!(!r.matches_num(1_600_000_000) && !r.matches_num(1_499_999_999));
        // Open-ended bounds.
        assert!(parse_filter(">=2", l).unwrap().matches_num(2 * SOL));
        assert!(!parse_filter(">=2", l).unwrap().matches_num(2 * SOL - 1));
        assert!(parse_filter("<=2", l).unwrap().matches_num(0));
        // A count axis reads its integer, not SOL.
        assert_eq!(parse_filter("5", AxisUnit::Count).unwrap().as_exact(), Some(5));
        assert_eq!(parse_filter("0.5", AxisUnit::Count), None);
        for bad in ["", "  ", "abc", "1.5–", "–1.6", "1.6–1.5", "1.5–1.5", "-1", "NaN", "inf"] {
            assert_eq!(parse_filter(bad, l), None, "{bad:?} must not parse");
        }
    }

    #[test]
    fn an_empty_plan_is_the_single_all_group() {
        let a = group_key(&tf(), &GroupPlan::default());
        let b = group_key(&TokenFingerprint::default(), &GroupPlan::default());
        assert_eq!(a, b);
        assert!(a.0.is_empty());
        assert_eq!(a.to_json(), serde_json::json!({}));
    }
}
