//! Fingerprint matcher — decides which token-creation shapes a token belongs to.
//!
//! A [`Fingerprint`] is a rule's target creation shape (backs a `fingerprints` DB
//! row; see `hunter/core/src/models/fingerprint.rs`). A token can match **many**.
//!
//! ```text
//! Fingerprint = wildcard | Criteria { axis -> predicate }
//! ```
//!
//! * **[`axis`]** is the registry: one `AxisDef` per matchable axis, carrying its
//!   wire key, kind, unit, match phase, definition, and the reader that pulls the
//!   observed value off a [`TokenFingerprint`]. Adding an axis touches that table
//!   and nothing here.
//! * An axis **absent** from the criteria map is not part of identity.
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
    /// The configured axes. Empty (without `wildcard`) matches nothing.
    #[serde(default)]
    pub criteria: Criteria,
    /// Per-metric-group fingerprint-side config (e.g. `m_flow_ix`). **Not** part
    /// of match identity — compiled into
    /// [`crate::metrics::flow_ix::FlowPatterns`] at reload.
    #[serde(default = "default_metric_config")]
    pub metric_config: serde_json::Value,
}

fn default_metric_config() -> serde_json::Value {
    serde_json::json!({})
}

impl Fingerprint {
    /// A fingerprint with no criteria — matches nothing until an axis or the
    /// wildcard is set. The one constructor callers build from.
    pub fn empty(id: FingerprintId) -> Self {
        Fingerprint {
            id,
            wildcard: false,
            criteria: Criteria::new(),
            metric_config: default_metric_config(),
        }
    }

    /// Whether any matchable criterion is configured. The matcher requires at least
    /// one, so an unconfigured fingerprint can never match everything.
    pub fn has_any_criterion(&self) -> bool {
        self.wildcard || !self.criteria.is_empty()
    }

    /// Whether anything resolves at creation. A wildcard counts: it answers for
    /// every token immediately and waits on nothing.
    pub fn has_instant_criterion(&self) -> bool {
        self.wildcard || self.criteria.has_instant()
    }

    /// Whether any configured axis defers to the settled creation slot. The producer
    /// reads this to decide whether an instant match must still wait for
    /// `FirstSlotSettled` before it fully resolves.
    ///
    /// A wildcard never defers — it has no axis to wait for.
    pub fn has_first_slot_criteria(&self) -> bool {
        !self.wildcard && self.criteria.has_deferred()
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
        if self.wildcard && !self.criteria.is_empty() {
            out.push(
                "a wildcard fingerprint matches every token, so it cannot also carry match \
                 axes — clear the axes or turn the wildcard off"
                    .into(),
            );
        }
        out.extend(self.criteria.problems());
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
    // A wildcard short-circuits every axis, including deferred ones — there is
    // nothing to wait for, so it resolves in the instant phase.
    if fp.wildcard {
        return true;
    }
    fp.criteria.iter().all(|(id, pred)| {
        if phase == MatchPhase::Instant && id.is_deferred() {
            return true; // not yet knowable — judged at Full
        }
        axis_matches(id, pred, tf)
    })
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

    #[test]
    fn configured_labels_collapses_the_empty_sequence() {
        assert_eq!(configured_labels(None), None);
        assert_eq!(configured_labels(Some(&[])), None);
        let one = ["A".to_string()];
        assert_eq!(configured_labels(Some(&one)), Some(&one[..]));
    }
}
