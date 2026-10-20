//! Fingerprint matcher — decides which token-creation shapes a token belongs to.
//!
//! A [`Fingerprint`] is a rule's target creation shape (backs a `fingerprints` DB
//! row; see `hunter/core/src/models/fingerprint.rs`). A token can match **many**.
//!
//! ```text
//! Fingerprint = (wildcard | shape OR shape OR ...)  AND NOT (shape OR shape OR ...)
//! shape       = Criteria { axis -> predicate }      (every axis must hold)
//! ```
//!
//! The include shapes are [`Fingerprint::criteria`] plus [`Fingerprint::alt_criteria`];
//! the exclude shapes are [`Fingerprint::exclude_criteria`]. A one-shape fingerprint
//! with no exclusions is the plain `Criteria` row, so every older row keeps its meaning.
//!
//! * **[`axis`]** is the registry: one `AxisDef` per matchable axis, carrying its
//!   wire key, kind, unit, match phase, definition, and the reader that pulls the
//!   observed value off a [`TokenFingerprint`]. Adding an axis touches that table
//!   and nothing here.
//! * An axis **absent** from a shape is not part of that shape.
//! * An exclude shape excludes only a token it has **fully judged**: an unknown value
//!   fails the shape, so the token is not excluded by it.
//! * A configured axis whose observed value is unknown **fails** — an unknown value
//!   is never shown to satisfy a bound.
//! * A fingerprint with no criteria matches **nothing**. "Every token" is spelled
//!   [`Fingerprint::wildcard`] out loud, because "the operator forgot to configure
//!   this" and "the operator means everything" are opposite readings of an empty
//!   row and must not share a spelling.
//!
//! **Two-phase resolution** ([`MatchPhase`]): an axis whose `AxisDef::phase` is
//! `FirstSlot` is trade-derived and only settles after the creation slot closes. At
//! [`MatchPhase::Instant`] (a `TokenCreated` event) those axes are skipped and the
//! fingerprint stays *pending*; at [`MatchPhase::Full`] (a `FirstSlotSettled` event)
//! every configured axis is judged. Which axes defer is read off the registry, so
//! nothing here enumerates them.

pub mod axis;
pub mod grammar;
pub mod identity_launches;
pub mod token;

pub use axis::{
    AxisDef, AxisId, AxisKind, AxisPhase, AxisPredicate, AxisUnit, Criteria, Span, SpanSet, AXES,
};
pub use grammar::{format_predicate, parse_predicate, parse_span_set, sol_text_to_lamports};
pub use token::{extract_lamports, lamports_to_sol, normalize_labels, sol_to_lamports, TokenFingerprint};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use uuid::Uuid;

/// A fingerprint's stable id (the `fingerprints.id` UUID). A pure 128-bit data
/// wrapper — ids are minted in the DB, never in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FingerprintId(pub Uuid);

/// A rule's target token-creation shape — the "WHO to trade" half of a generic rule
/// (the "WHEN"/"HOW" halves are [`crate::rule_params`] + the rule row). The engine
/// receives these on a `RulesReloaded` event, already converted from the DB row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub id: FingerprintId,
    /// Match EVERY token, ignoring every axis.
    ///
    /// A rule always needs a fingerprint, but not every rule is about a creation
    /// shape — one deciding purely on what the tape is doing has none to name.
    /// Deliberately not an unbounded range on some axis and not an empty criteria
    /// map: both would make "unconfigured" and "everything" the same state.
    #[serde(default)]
    pub wildcard: bool,
    /// The first include shape. Empty (without `wildcard`) matches nothing.
    #[serde(default)]
    pub criteria: Criteria,
    /// Further include shapes: a token passes on [`Self::criteria`] OR any of these.
    /// Never set beside `wildcard`, and never without `criteria`; stored canonical
    /// ([`Self::canonicalize`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_criteria: Vec<Criteria>,
    /// Exclude shapes: a token matching ANY of these is rejected, whatever it
    /// included on. Allowed beside `wildcard` ("every token except ...").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_criteria: Vec<Criteria>,
    /// The fingerprint's tags: named trade lists, `{name: definition}`
    /// ([`crate::metrics::tags::config`]). **Not** part of match identity — compiled
    /// once per reload.
    #[serde(default = "default_tags")]
    pub tags: serde_json::Value,
}

fn default_tags() -> serde_json::Value {
    serde_json::json!({})
}

/// One shape's sort key — its canonical JSON. `Criteria` is a `BTreeMap` in registry
/// order, so equal shapes serialise to equal text.
fn shape_key(c: &Criteria) -> String {
    serde_json::to_string(c).unwrap_or_default()
}

/// Sorted by [`shape_key`] with duplicates dropped — the one stored order of a shape
/// list, so two spellings of one token set are one row.
fn canonical_shapes(mut shapes: Vec<Criteria>) -> Vec<Criteria> {
    shapes.sort_by_cached_key(shape_key);
    shapes.dedup();
    shapes
}

impl Fingerprint {
    /// A fingerprint with no criteria — matches nothing until an axis or the
    /// wildcard is set. The one constructor callers build from.
    pub fn empty(id: FingerprintId) -> Self {
        Fingerprint {
            id,
            wildcard: false,
            criteria: Criteria::new(),
            alt_criteria: Vec::new(),
            exclude_criteria: Vec::new(),
            tags: default_tags(),
        }
    }

    /// The include shapes: [`Self::criteria`] (when configured) then
    /// [`Self::alt_criteria`]. Empty for a wildcard.
    pub fn include_shapes(&self) -> impl Iterator<Item = &Criteria> {
        std::iter::once(&self.criteria).filter(|c| !c.is_empty()).chain(self.alt_criteria.iter())
    }

    /// Every configured shape, include and exclude — for a reader that needs each
    /// axis the fingerprint names (the name-reuse build list).
    pub fn all_shapes(&self) -> impl Iterator<Item = &Criteria> {
        self.include_shapes().chain(self.exclude_criteria.iter())
    }

    /// The builds (exact `ix_labels` sequences) whose launches the
    /// `name_reuse_count` axis counts: one per shape that reads that axis and pins
    /// a label sequence. The one reader, for the engine reload and both primers.
    pub fn name_reuse_builds(&self) -> impl Iterator<Item = &[String]> {
        self.all_shapes()
            .filter(|c| c.contains(AxisId::NameReuseCount))
            .filter_map(|c| match c.get(AxisId::IxLabels) {
                Some(AxisPredicate::Sequence { labels }) => Some(labels.as_slice()),
                _ => None,
            })
    }

    /// Whether the fingerprint has more than the single-shape form — an alternative
    /// include shape or any exclusion. A reader that understands only one `Criteria`
    /// (an SQL pre-filter, a group-key match) checks this before trusting `criteria`.
    pub fn is_multi_shape(&self) -> bool {
        !self.alt_criteria.is_empty() || !self.exclude_criteria.is_empty()
    }

    /// Store the shape lists in their one canonical order: every include shape
    /// sorted and de-duplicated, the first in `criteria` and the rest in
    /// `alt_criteria`; the exclude shapes sorted and de-duplicated. Empty shapes
    /// are kept so [`Self::problems`] can name them rather than silently widen the
    /// row. The write edge calls this; [`Self::problems`] refuses a row that skipped it.
    pub fn canonicalize(&mut self) {
        if !self.alt_criteria.is_empty() && !self.criteria.is_empty() {
            let mut all = std::mem::take(&mut self.alt_criteria);
            all.push(std::mem::take(&mut self.criteria));
            let mut all = canonical_shapes(all).into_iter();
            self.criteria = all.next().unwrap_or_default();
            self.alt_criteria = all.collect();
        }
        self.exclude_criteria = canonical_shapes(std::mem::take(&mut self.exclude_criteria));
    }

    /// Whether any matchable criterion is configured. The matcher requires at least
    /// one, so an unconfigured fingerprint can never match everything.
    pub fn has_any_criterion(&self) -> bool {
        self.wildcard || !self.criteria.is_empty()
    }

    /// Whether anything resolves at creation. A wildcard counts: it answers for
    /// every token immediately and waits on nothing.
    pub fn has_instant_criterion(&self) -> bool {
        self.wildcard || self.include_shapes().any(Criteria::has_instant)
    }

    /// Whether any configured axis defers to the settled creation slot. The producer
    /// reads this to decide whether an instant match must still wait for
    /// `FirstSlotSettled` before it fully resolves.
    ///
    /// A wildcard include waits for nothing, but an exclude shape with a deferred
    /// axis still does: until the slot settles it cannot say whether it rejects.
    pub fn has_first_slot_criteria(&self) -> bool {
        (!self.wildcard && self.include_shapes().any(Criteria::has_deferred))
            || self.exclude_criteria.iter().any(Criteria::has_deferred)
    }

    /// Every way this fingerprint is unusable, as operator-facing sentences.
    /// Empty ⇒ valid. **The one gate**, shared by the HTTP write edge (for a 400)
    /// and the repo insert/update (backstop for non-HTTP writers like sweep
    /// promotion).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.has_any_criterion() {
            out.push(
                "a fingerprint must configure at least one axis, or be marked as matching \
                 every token"
                    .into(),
            );
        }
        // A wildcard already answers the match for every token, so an axis beside it
        // is a contradiction the matcher resolves silently in favour of the wildcard
        // — the operator would read the axes and expect them to narrow it.
        if self.wildcard && (!self.criteria.is_empty() || !self.alt_criteria.is_empty()) {
            out.push(
                "a wildcard fingerprint matches every token, so it cannot also carry match \
                 axes — clear the axes or turn the wildcard off"
                    .into(),
            );
        }
        if !self.wildcard && self.criteria.is_empty() && !self.alt_criteria.is_empty() {
            out.push("the first include shape is empty — remove it or give it an axis".into());
        }
        out.extend(self.criteria.problems());
        for (i, shape) in self.alt_criteria.iter().enumerate() {
            let label = shape_label(i + 1);
            if shape.is_empty() {
                out.push(format!("include shape {label} configures no axis — give it one or remove it"));
            }
            out.extend(shape.problems().into_iter().map(|p| format!("include shape {label}: {p}")));
        }
        for (i, shape) in self.exclude_criteria.iter().enumerate() {
            let label = format!("X{}", i + 1);
            if shape.is_empty() {
                out.push(format!(
                    "exclude shape {label} configures no axis, so it would exclude every token — \
                     give it an axis or remove it"
                ));
            }
            out.extend(shape.problems().into_iter().map(|p| format!("exclude shape {label}: {p}")));
        }
        // A stored order other than the canonical one is a second spelling of the
        // same token set, which `find_or_create` would fork into a second row.
        let mut canonical = self.clone();
        canonical.canonicalize();
        if canonical.criteria != self.criteria
            || canonical.alt_criteria != self.alt_criteria
            || canonical.exclude_criteria != self.exclude_criteria
        {
            out.push("the shape lists are not in canonical order (sorted, no duplicates)".into());
        }
        out
    }

    /// [`Self::problems`] as a single `Result`, for callers that only need the first
    /// reason.
    pub fn validate(&self) -> Result<(), String> {
        match self.problems().first() {
            None => Ok(()),
            Some(first) => Err(first.clone()),
        }
    }
}

/// An include shape's operator-facing letter: `A` is [`Fingerprint::criteria`], `B`
/// onwards the [`Fingerprint::alt_criteria`]. Exclude shapes read `X1`, `X2`, ...
/// Mirrored by the TS editor's shape labels.
fn shape_label(index: usize) -> String {
    match u8::try_from(index).ok().filter(|i| *i < 26) {
        Some(i) => char::from(b'A' + i).to_string(),
        None => format!("#{}", index + 1),
    }
}

/// Which axes a match judges. Deferred axes settle only after the creation slot
/// closes, so matching is two-phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    /// `TokenCreated`: judge only the axes known at creation. A fingerprint with a
    /// deferred axis stays *pending*.
    Instant,
    /// `FirstSlotSettled`: judge every configured axis.
    Full,
}

/// Whether a token satisfies a fingerprint at the given phase.
///
/// The whole matcher is this loop: the registry says how to read each axis and
/// whether it defers, and the predicate says whether the value passes. Nothing here
/// knows what an axis *means*.
pub fn matches_phase(fp: &Fingerprint, tf: &TokenFingerprint, phase: MatchPhase) -> bool {
    if !fp.has_any_criterion() {
        return false;
    }
    // A wildcard include short-circuits every axis, including deferred ones — there
    // is nothing to wait for, so it resolves in the instant phase.
    let included = fp.wildcard || fp.include_shapes().any(|c| shape_matches(c, tf, phase));
    included && !fp.exclude_criteria.iter().any(|c| shape_excludes(c, tf, phase))
}

/// One shape: every configured axis holds. At [`MatchPhase::Instant`] a deferred
/// axis is not yet knowable and passes — it is judged at `Full`.
fn shape_matches(shape: &Criteria, tf: &TokenFingerprint, phase: MatchPhase) -> bool {
    shape.iter().all(|(id, pred)| {
        if phase == MatchPhase::Instant && id.is_deferred() {
            return true;
        }
        axis_matches(id, pred, tf)
    })
}

/// Whether an exclude shape rejects the token. It rejects only what it has fully
/// judged: at `Instant` a shape with a deferred axis is undecided, and the
/// fingerprint is pending anyway ([`Fingerprint::has_first_slot_criteria`]), so the
/// `Full` phase decides. An unknown value fails the shape, so it does not reject.
fn shape_excludes(shape: &Criteria, tf: &TokenFingerprint, phase: MatchPhase) -> bool {
    (phase == MatchPhase::Full || !shape.has_deferred()) && shape_matches(shape, tf, MatchPhase::Full)
}

/// One axis of the match. A configured axis with no observed value fails: this is
/// the fail-closed direction, and it is the only one that cannot arm a rule on a
/// token nobody screened.
fn axis_matches(id: AxisId, pred: &AxisPredicate, tf: &TokenFingerprint) -> bool {
    // Routed by the predicate's AXIS KIND, not its variant, so a new numeric shape
    // (`Spans` for `!=` / `|`) reads the same value through the same reader — a
    // variant this loop had never heard of would otherwise fall through to
    // "matches nothing" while the row still read as a numeric gate.
    match pred.kind() {
        AxisKind::Numeric => id.read_num(tf).is_some_and(|v| pred.matches_num(v)),
        AxisKind::Sequence => id.read_seq(tf).is_some_and(|s| pred.matches_seq(s)),
    }
}

/// Full match (every configured axis, incl. deferred). Use once the creation slot
/// has settled.
pub fn matches(fp: &Fingerprint, tf: &TokenFingerprint) -> bool {
    matches_phase(fp, tf, MatchPhase::Full)
}

/// All fingerprints the token matches at `phase`, in input order (multi-match). The
/// engine calls this with [`MatchPhase::Instant`] at `TokenCreated` (arm the instant
/// hits, mark deferred ones pending) and [`MatchPhase::Full`] at `FirstSlotSettled`.
pub fn match_all(
    fps: &[Fingerprint],
    tf: &TokenFingerprint,
    phase: MatchPhase,
) -> SmallVec<[FingerprintId; 4]> {
    fps.iter().filter(|fp| matches_phase(fp, tf, phase)).map(|fp| fp.id).collect()
}

/// **The one place that decides whether a label sequence is configured.** An empty
/// collection is the same sentinel as absent, so it must collapse everywhere or two
/// readers will disagree about whether a fingerprint has any criteria at all.
pub fn configured_labels(labels: Option<&[String]>) -> Option<&[String]> {
    labels.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp_id(n: u128) -> FingerprintId {
        FingerprintId(Uuid::from_u128(n))
    }

    fn blank_fp() -> Fingerprint {
        Fingerprint::empty(fp_id(1))
    }

    fn with(id: AxisId, pred: AxisPredicate) -> Fingerprint {
        let mut fp = blank_fp();
        fp.criteria.insert(id, pred);
        fp
    }

    const SOL: u128 = 1_000_000_000;

    #[test]
    fn no_criteria_never_matches() {
        let fp = blank_fp();
        let tf = TokenFingerprint::default();
        assert!(!matches(&fp, &tf));
        assert!(!matches_phase(&fp, &tf, MatchPhase::Instant));
        assert!(match_all(std::slice::from_ref(&fp), &tf, MatchPhase::Full).is_empty());
        assert!(fp.validate().is_err());
    }

    #[test]
    fn a_wildcard_matches_everything_and_an_empty_row_still_matches_nothing() {
        let empty = blank_fp();
        let any = Fingerprint { wildcard: true, ..blank_fp() };
        let tf = TokenFingerprint {
            cu_limit: Some(123_456),
            ix_labels: vec!["Pump.Fun: Create".into()],
            ..Default::default()
        };
        assert!(!matches(&empty, &tf), "an unconfigured row arms on nothing");
        assert!(matches(&any, &tf), "a wildcard arms on everything");
        // It resolves at creation — there is no deferred axis to wait for.
        assert!(matches_phase(&any, &tf, MatchPhase::Instant));
        assert!(any.has_instant_criterion() && !any.has_first_slot_criteria());
        assert!(any.validate().is_ok());
    }

    #[test]
    fn a_wildcard_cannot_also_carry_an_axis() {
        let mut fp = Fingerprint { wildcard: true, ..blank_fp() };
        fp.criteria.insert(AxisId::CuLimit, AxisPredicate::exact(1));
        assert!(fp.validate().is_err());
    }

    #[test]
    fn exact_axes_reject_a_wrong_or_missing_value() {
        let mut fp = with(AxisId::CuLimit, AxisPredicate::exact(200_000));
        fp.criteria.insert(AxisId::CuPrice, AxisPredicate::exact(1_000));
        let mut tf = TokenFingerprint { cu_limit: Some(200_000), cu_price: Some(1_000), ..Default::default() };
        assert!(matches(&fp, &tf));
        tf.cu_price = Some(999);
        assert!(!matches(&fp, &tf), "wrong value");
        tf.cu_price = None;
        assert!(!matches(&fp, &tf), "a configured axis with no observed value fails closed");
    }

    #[test]
    fn a_range_selects_a_window_an_exact_axis_would_split() {
        // [1.0, 1.6] SOL — the kind of window a fixed-width bucket could not name.
        let fp = with(AxisId::InitBuyLamports, AxisPredicate::range(Some(SOL), Some(SOL * 16 / 10)));
        let at = |sol_x10: u64| TokenFingerprint {
            init_buy_lamports: Some(sol_x10 * 100_000_000),
            ..Default::default()
        };
        assert!(matches(&fp, &at(10)) && matches(&fp, &at(15)) && matches(&fp, &at(16)));
        assert!(!matches(&fp, &at(9)) && !matches(&fp, &at(17)));
        // An open bound is a one-sided gate.
        let open = with(AxisId::InitBuyLamports, AxisPredicate::range(Some(SOL * 2), None));
        assert!(matches(&open, &at(100)) && !matches(&open, &at(19)));
    }

    /// The `u64::MAX` "fill at any price" ceiling is a real launch setting, so it
    /// must be nameable as itself — and must not be swept into a neighbouring
    /// window by accident. This is the case that broke when the axis was a `BIGINT`.
    #[test]
    fn a_no_limit_ceiling_is_matchable_as_itself() {
        let ceiling = u128::from(u64::MAX);
        let fp = with(AxisId::MaxCostLamports, AxisPredicate::exact(ceiling));
        let mut tf = TokenFingerprint { max_cost_lamports: Some(u64::MAX), ..Default::default() };
        assert!(matches(&fp, &tf), "the ceiling must satisfy an axis naming it");
        tf.max_cost_lamports = Some(u64::MAX - 1);
        assert!(!matches(&fp, &tf), "one lamport below is a different value");

        // A real amount's axis is not satisfied by the ceiling.
        let real = with(AxisId::MaxCostLamports, AxisPredicate::exact(1_515_000_000));
        tf.max_cost_lamports = Some(u64::MAX);
        assert!(!matches(&real, &tf));
        tf.max_cost_lamports = Some(1_515_000_000);
        assert!(matches(&real, &tf));
    }

    #[test]
    fn ix_labels_match_exact_ordered() {
        let fp = with(
            AxisId::IxLabels,
            AxisPredicate::Sequence { labels: vec!["A".into(), "B".into(), "C".into()] },
        );
        let with_labels = |l: &[&str]| TokenFingerprint {
            ix_labels: l.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert!(matches(&fp, &with_labels(&["A", "B", "C"])));
        assert!(!matches(&fp, &with_labels(&["A", "C", "B"])), "wrong order");
        assert!(!matches(&fp, &with_labels(&["A", "B"])), "subset");
        assert!(!matches(&fp, &with_labels(&["A", "B", "C", "D"])), "superset");
        assert!(!matches(&fp, &with_labels(&[])), "no labels");
    }

    /// The two new axes: a derived count and the engine's own creator tally.
    #[test]
    fn ix_count_and_prior_launches_are_ordinary_axes() {
        let fp = with(AxisId::IxCount, AxisPredicate::range(Some(3), Some(5)));
        let tf = |n: usize| TokenFingerprint {
            ix_labels: (0..n).map(|i| format!("ix{i}")).collect(),
            ..Default::default()
        };
        assert!(matches(&fp, &tf(3)) && matches(&fp, &tf(5)));
        assert!(!matches(&fp, &tf(2)) && !matches(&fp, &tf(6)));

        // `prior_launches == 0` is a real value (a first-time creator) and must not
        // be satisfied by an UNKNOWN creator — that inversion is the whole hazard.
        let first_time = with(AxisId::PriorLaunches, AxisPredicate::exact(0));
        assert!(matches(&first_time, &TokenFingerprint { prior_launches: Some(0), ..Default::default() }));
        assert!(!matches(&first_time, &TokenFingerprint { prior_launches: None, ..Default::default() }));
        assert!(!matches(&first_time, &TokenFingerprint { prior_launches: Some(1), ..Default::default() }));
    }

    #[test]
    fn deferred_axes_resolve_in_two_phases() {
        let fp = with(AxisId::FirstSlotBuyLamports, AxisPredicate::range(Some(SOL * 2), Some(SOL * 3)));
        assert!(fp.has_first_slot_criteria());
        assert!(!fp.has_instant_criterion());

        // Instant: the value is not knowable yet, so the axis is skipped and the
        // fingerprint stays pending rather than being rejected.
        let pending = TokenFingerprint::default();
        assert!(matches_phase(&fp, &pending, MatchPhase::Instant));
        assert!(!matches_phase(&fp, &pending, MatchPhase::Full), "unknown at Full fails");

        let settled = TokenFingerprint { first_slot_buy_lamports: Some(2_500_000_000), ..Default::default() };
        assert!(matches_phase(&fp, &settled, MatchPhase::Full));
        let out = TokenFingerprint { first_slot_buy_lamports: Some(9_000_000_000), ..Default::default() };
        assert!(!matches_phase(&fp, &out, MatchPhase::Full));
    }

    #[test]
    fn an_instant_axis_is_still_judged_at_the_instant_phase() {
        let mut fp = with(AxisId::CuLimit, AxisPredicate::exact(200_000));
        fp.criteria.insert(AxisId::FirstSlotSellLamports, AxisPredicate::exact(SOL));
        let mut tf = TokenFingerprint { cu_limit: Some(199_999), ..Default::default() };
        assert!(!matches_phase(&fp, &tf, MatchPhase::Instant), "we do not wait to reject");
        tf.cu_limit = Some(200_000);
        assert!(matches_phase(&fp, &tf, MatchPhase::Instant), "pending pass");
    }

    /// A `!=` / `|` axis is judged by the SAME loop a plain range is — routed by
    /// axis kind, so the reader and the fail-closed rule are unchanged and only the
    /// set of accepted values is wider.
    #[test]
    fn a_gap_axis_is_matched_like_any_other_numeric_axis() {
        let fp = with(AxisId::IxCount, AxisPredicate::not_range(Some(3), Some(3)));
        for (len, want) in [(2usize, true), (3, false), (4, true)] {
            let tf = TokenFingerprint {
                ix_labels: vec!["ix".to_string(); len],
                ..Default::default()
            };
            assert_eq!(matches(&fp, &tf), want, "ix_count {len}");
        }
        // A configured axis with no observed value still FAILS closed, exactly as a
        // range does — the gap shape widens what passes, never what is knowable.
        let fp = with(AxisId::PriorLaunches, AxisPredicate::not_range(Some(0), Some(0)));
        assert!(!matches(&fp, &TokenFingerprint { prior_launches: None, ..Default::default() }));
    }

    #[test]
    fn multi_match_returns_all_hits_in_input_order() {
        let mut a = with(AxisId::CuLimit, AxisPredicate::exact(200_000));
        a.id = fp_id(10);
        let mut b = with(AxisId::CuLimit, AxisPredicate::exact(999_999));
        b.id = fp_id(20);
        // A coarse window and a tight pin can both claim the same token — each axis
        // carries its own predicate, so there is no row-wide precision to reconcile.
        let mut c = with(AxisId::InitBuyLamports, AxisPredicate::range(Some(SOL), Some(SOL * 2)));
        c.id = fp_id(30);
        let mut d = with(AxisId::InitBuyLamports, AxisPredicate::exact(SOL * 15 / 10));
        d.id = fp_id(40);

        let tf = TokenFingerprint {
            cu_limit: Some(200_000),
            init_buy_lamports: Some(1_500_000_000),
            ..Default::default()
        };
        let hits = match_all(&[a.clone(), b, c.clone(), d.clone()], &tf, MatchPhase::Full);
        assert_eq!(hits.as_slice(), &[a.id, c.id, d.id]);
    }

    fn shape(axis: AxisId, pred: AxisPredicate) -> Criteria {
        Criteria::new().with(axis, pred)
    }

    fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("ix{i}")).collect()
    }

    /// The whitelist: `(5ix · cu_limit=400K) OR (4ix · init=1)`. A token passes on
    /// either shape, and a token half-matching both passes on neither.
    #[test]
    fn a_token_passes_on_any_include_shape() {
        let mut fp = blank_fp();
        fp.criteria = Criteria::new()
            .with(AxisId::IxCount, AxisPredicate::exact(5))
            .with(AxisId::CuLimit, AxisPredicate::exact(400_000));
        fp.alt_criteria = vec![Criteria::new()
            .with(AxisId::IxCount, AxisPredicate::exact(4))
            .with(AxisId::InitBuyLamports, AxisPredicate::exact(SOL))];
        fp.canonicalize();
        assert!(fp.validate().is_ok(), "{:?}", fp.problems());
        let tf = |n: usize, cu: u64, init: u64| TokenFingerprint {
            ix_labels: labels(n),
            cu_limit: Some(cu),
            init_buy_lamports: Some(init),
            ..Default::default()
        };
        assert!(matches(&fp, &tf(5, 400_000, 7)), "shape A");
        assert!(matches(&fp, &tf(4, 1, SOL as u64)), "shape B");
        assert!(!matches(&fp, &tf(5, 200_000, SOL as u64)), "5ix but not A's cu_limit");
        assert!(!matches(&fp, &tf(6, 400_000, SOL as u64)), "neither");
    }

    /// The blacklist: every token except 7ix.
    #[test]
    fn a_wildcard_minus_an_exclude_shape_is_everything_else() {
        let mut fp = Fingerprint { wildcard: true, ..blank_fp() };
        fp.exclude_criteria = vec![shape(AxisId::IxCount, AxisPredicate::exact(7))];
        assert!(fp.validate().is_ok(), "{:?}", fp.problems());
        let tf = |n: usize| TokenFingerprint { ix_labels: labels(n), ..Default::default() };
        assert!(matches(&fp, &tf(5)) && matches(&fp, &tf(6)));
        assert!(!matches(&fp, &tf(7)));
        assert!(!matches_phase(&fp, &tf(7), MatchPhase::Instant), "an instant exclusion rejects at once");
        // An unknown value cannot be shown to be the excluded shape, so it passes.
        let unknown = Fingerprint { exclude_criteria: vec![shape(AxisId::CuLimit, AxisPredicate::exact(1))], ..fp };
        assert!(matches(&unknown, &TokenFingerprint::default()));
    }

    #[test]
    fn an_exclude_shape_narrows_an_include_shape() {
        let mut fp = with(AxisId::IxCount, AxisPredicate::exact(5));
        fp.exclude_criteria = vec![Criteria::new()
            .with(AxisId::IxCount, AxisPredicate::exact(5))
            .with(AxisId::CuPrice, AxisPredicate::exact(250_000))];
        let tf = |price: u64| TokenFingerprint {
            ix_labels: labels(5),
            cu_price: Some(price),
            ..Default::default()
        };
        assert!(matches(&fp, &tf(1)));
        assert!(!matches(&fp, &tf(250_000)));
    }

    /// A deferred axis in an exclude shape makes even a wildcard wait: until the
    /// slot settles the shape cannot say whether it rejects.
    #[test]
    fn a_deferred_exclude_shape_defers_the_whole_fingerprint() {
        let mut fp = Fingerprint { wildcard: true, ..blank_fp() };
        assert!(!fp.has_first_slot_criteria());
        fp.exclude_criteria = vec![shape(AxisId::FirstSlotBuyLamports, AxisPredicate::range(Some(SOL), None))];
        assert!(fp.has_first_slot_criteria());
        let pending = TokenFingerprint::default();
        assert!(matches_phase(&fp, &pending, MatchPhase::Instant), "undecided at creation");
        let big = TokenFingerprint { first_slot_buy_lamports: Some(2 * SOL as u64), ..Default::default() };
        assert!(!matches_phase(&fp, &big, MatchPhase::Full));
        let small = TokenFingerprint { first_slot_buy_lamports: Some(1), ..Default::default() };
        assert!(matches_phase(&fp, &small, MatchPhase::Full));
    }

    /// Two orders of the same shapes are one stored row, and a hand-written row in
    /// another order is refused rather than normalised on read.
    #[test]
    fn shape_lists_have_one_canonical_order() {
        let a = shape(AxisId::IxCount, AxisPredicate::exact(5));
        let b = shape(AxisId::IxCount, AxisPredicate::exact(4));
        let mut one = Fingerprint { criteria: a.clone(), alt_criteria: vec![b.clone(), a.clone()], ..blank_fp() };
        let mut two = Fingerprint { criteria: b.clone(), alt_criteria: vec![a.clone()], ..blank_fp() };
        assert!(two.validate().is_err() || one.validate().is_err(), "at most one order is canonical");
        one.canonicalize();
        two.canonicalize();
        assert_eq!(one, two);
        assert_eq!(one.alt_criteria.len(), 1, "the duplicate is dropped");
        assert!(one.validate().is_ok());
    }

    #[test]
    fn malformed_shape_lists_are_named() {
        let five = shape(AxisId::IxCount, AxisPredicate::exact(5));
        let wild_alt = Fingerprint { wildcard: true, alt_criteria: vec![five.clone()], ..blank_fp() };
        assert!(wild_alt.validate().is_err(), "a wildcard carries no include shape");
        let empty_exclude = Fingerprint { exclude_criteria: vec![Criteria::new()], ..with(AxisId::IxCount, AxisPredicate::exact(4)) };
        assert!(empty_exclude.validate().is_err(), "an empty exclude shape would reject everything");
        let only_exclude = Fingerprint { exclude_criteria: vec![five], ..blank_fp() };
        assert!(only_exclude.validate().is_err(), "nothing included");
    }

    #[test]
    fn configured_labels_collapses_the_empty_sequence() {
        assert_eq!(configured_labels(None), None);
        assert_eq!(configured_labels(Some(&[])), None);
        let one = ["A".to_string()];
        assert_eq!(configured_labels(Some(&one)), Some(&one[..]));
    }
}
