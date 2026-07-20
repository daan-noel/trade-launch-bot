//! Fingerprint matcher — decides which token-creation shapes a token belongs to.
//!
//! A [`Fingerprint`] is a rule's target creation shape (backs a `fingerprints`
//! DB row; see `hunter/core/src/models/fingerprint.rs`). A token can match
//! **many** fingerprints. This module is the SSOT for the match semantics —
//! ported verbatim from the legacy `entry/mod.rs` `check_*` sets so the redesign
//! makes identical arming decisions:
//!
//! * **Exact** axes: `cu_limit`, `cu_price`, `ix_labels` (exact **ordered**
//!   sequence — same length, same string at every position).
//! * **Bucket-matched** axes (SOL space, via this fingerprint's own
//!   `bucket_size_amount` and the [`crate::grouping::same_bucket`] SSOT):
//!   `init_buy`, `max_cost`, `spendable_in`, `first_slot_buy`, `first_slot_sell`.
//! * A `None` axis is **not part of the fingerprint's identity** (ignored).
//! * A fingerprint with **no** configured axis never matches (a misconfigured
//!   all-`None` fingerprint can't snipe every token).
//!
//! **Two-phase first-slot resolution** ([`MatchPhase`]): the two `first_slot_*`
//! axes are trade-derived and only settle after the creation slot closes. At
//! [`MatchPhase::Instant`] (a `TokenCreated` event) only the instant axes are
//! judged; the full match — including the first-slot axes — is judged at
//! [`MatchPhase::Full`] (a `FirstSlotSettled` event). The producer owns slot-close
//! detection (plan §2.2); this module just exposes both judgments.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use uuid::Uuid;

use crate::grouping::{same_bucket, TokenFingerprint, LAMPORTS_PER_SOL_F64};

/// A fingerprint's stable id (the `fingerprints.id` UUID). A pure 128-bit data
/// wrapper — ids are minted in the DB, never in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FingerprintId(pub Uuid);

/// A rule's target token-creation shape — the "WHO to trade" half of a generic
/// rule (the "WHEN"/"HOW" halves are [`crate::rule_params`] + the rule row). The
/// engine receives these on a `RulesReloaded` event, already converted from the
/// [`crate::grouping::TokenFingerprint`]-free DB row. Lamports (`i64`) at rest;
/// the bucket match converts to SOL via [`Fingerprint::*_sol`] accessors.
///
/// Every SOL axis is bucket-matched at the row's own [`Self::bucket_size_amount`]
/// (deliberately per-fingerprint — a coarse fingerprint and a fine one can share
/// the same token). The exact axes ignore the width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub id: FingerprintId,
    // ── Exact axes ──────────────────────────────────────────────────────────
    /// Exact compute-unit limit of the creation tx.
    pub cu_limit: Option<i64>,
    /// Exact compute-unit price of the creation tx.
    pub cu_price: Option<i64>,
    /// Exact **ordered** instruction-label sequence of the creation tx. `None`
    /// (or empty) = not part of identity.
    pub ix_labels: Option<Vec<String>>,
    // ── Bucket-matched SOL axes (lamports at rest) ─────────────────────────────
    /// Creator's initial dev-buy size.
    pub init_buy_lamports: Option<i64>,
    /// `max_sol_cost` arg of the initial buy instruction.
    pub max_cost_lamports: Option<i64>,
    /// Spendable SOL the creator wallet held going in.
    pub spendable_lamports_in: Option<i64>,
    /// Sum of buy SOL in the creation slot (first-slot axis — deferred).
    pub first_slot_buy_lamports: Option<i64>,
    /// Sum of sell SOL in the creation slot (first-slot axis — deferred).
    pub first_slot_sell_lamports: Option<i64>,
    /// SOL width every bucket-matched axis on this row uses.
    pub bucket_size_amount: f64,
    /// Per-metric-group fingerprint-side config (e.g. `m_flow_split`). Not part
    /// of match identity — compiled into [`crate::metrics::flow_split::FlowPatterns`]
    /// at reload. Default `{}` for older event-log / test fixtures.
    #[serde(default = "default_metric_config")]
    pub metric_config: serde_json::Value,
}

fn default_metric_config() -> serde_json::Value {
    serde_json::json!({})
}

impl Fingerprint {
    pub fn init_buy_sol(&self) -> Option<f64> {
        self.init_buy_lamports.map(lamports_to_sol)
    }
    pub fn max_cost_sol(&self) -> Option<f64> {
        self.max_cost_lamports.map(lamports_to_sol)
    }
    pub fn spendable_sol_in(&self) -> Option<f64> {
        self.spendable_lamports_in.map(lamports_to_sol)
    }
    pub fn first_slot_buy_sol(&self) -> Option<f64> {
        self.first_slot_buy_lamports.map(lamports_to_sol)
    }
    pub fn first_slot_sell_sol(&self) -> Option<f64> {
        self.first_slot_sell_lamports.map(lamports_to_sol)
    }

    /// Whether any matchable criterion is configured. The matcher requires at
    /// least one so an all-`None` fingerprint can never match everything.
    pub fn has_any_criterion(&self) -> bool {
        self.has_instant_criterion() || self.has_first_slot_criteria()
    }

    /// Whether any **instant** (creation-time) axis is configured.
    pub fn has_instant_criterion(&self) -> bool {
        self.cu_limit.is_some()
            || self.cu_price.is_some()
            || self.configured_ix_labels().is_some()
            || self.init_buy_lamports.is_some()
            || self.max_cost_lamports.is_some()
            || self.spendable_lamports_in.is_some()
    }

    /// Whether either **first-slot** (deferred) axis is configured. The producer
    /// uses this to decide whether an instant match must still wait for
    /// `FirstSlotSettled` before it fully resolves (plan §2.2).
    pub fn has_first_slot_criteria(&self) -> bool {
        self.first_slot_buy_lamports.is_some() || self.first_slot_sell_lamports.is_some()
    }

    /// The configured ix-label sequence, treating `Some(empty)` as absent (mirrors
    /// the legacy `check_instruction_labels`: an empty rule label list is inert).
    fn configured_ix_labels(&self) -> Option<&[String]> {
        self.ix_labels.as_deref().filter(|v| !v.is_empty())
    }
}

/// Which axes a match judges. First-slot axes only settle after the creation
/// slot closes, so matching is two-phase (plan §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    /// `TokenCreated`: judge only the instant axes (first-slot axes not yet
    /// known — a fingerprint with first-slot criteria stays *pending*).
    Instant,
    /// `FirstSlotSettled`: judge every configured axis (the full match).
    Full,
}

/// Whether a token satisfies a fingerprint at the given phase. Requires ≥1
/// configured axis (never match-everything). At [`MatchPhase::Instant`] the
/// first-slot axes are skipped; at [`MatchPhase::Full`] every configured axis —
/// including first-slot ones — must be satisfied (a configured first-slot axis
/// with an unknown token value fails, exactly like the legacy full `CRITERIA`).
pub fn matches_phase(fp: &Fingerprint, tf: &TokenFingerprint, phase: MatchPhase) -> bool {
    if !fp.has_any_criterion() {
        return false;
    }
    let w = fp.bucket_size_amount;

    // ── Exact axes ──
    if let Some(cu) = fp.cu_limit {
        if tf.cu_limit != Some(cu) {
            return false;
        }
    }
    if let Some(cu) = fp.cu_price {
        if tf.cu_price != Some(cu) {
            return false;
        }
    }
    if let Some(labels) = fp.configured_ix_labels() {
        // Exact ordered match: same length, same string at every position.
        if labels.len() != tf.ix_labels.len()
            || labels.iter().zip(tf.ix_labels.iter()).any(|(a, b)| a != b)
        {
            return false;
        }
    }

    // ── Bucket-matched instant SOL axes ──
    if !bucket_axis(fp.init_buy_sol(), tf.initial_buy_sol, w) {
        return false;
    }
    if !bucket_axis(fp.max_cost_sol(), tf.max_cost_lamports.map(lamports_to_sol), w) {
        return false;
    }
    if !bucket_axis(fp.spendable_sol_in(), tf.spendable_lamports_in.map(lamports_to_sol), w) {
        return false;
    }

    // ── First-slot axes (deferred: only judged at Full) ──
    if phase == MatchPhase::Full {
        if !bucket_axis(fp.first_slot_buy_sol(), tf.first_slot_buy_sol, w) {
            return false;
        }
        if !bucket_axis(fp.first_slot_sell_sol(), tf.first_slot_sell_sol, w) {
            return false;
        }
    }

    true
}

/// Full match (every configured axis, incl. first-slot). Use once the creation
/// slot has settled.
pub fn matches(fp: &Fingerprint, tf: &TokenFingerprint) -> bool {
    matches_phase(fp, tf, MatchPhase::Full)
}

/// All fingerprints the token matches at `phase`, in input order (multi-match).
/// The engine calls this with [`MatchPhase::Instant`] at `TokenCreated` (arm the
/// instant hits + mark first-slot ones pending) and [`MatchPhase::Full`] at
/// `FirstSlotSettled` (resolve the pending ones).
pub fn match_all(
    fps: &[Fingerprint],
    tf: &TokenFingerprint,
    phase: MatchPhase,
) -> SmallVec<[FingerprintId; 4]> {
    fps.iter().filter(|fp| matches_phase(fp, tf, phase)).map(|fp| fp.id).collect()
}

/// One bucket-matched axis: `None` fingerprint value = not configured (always
/// satisfied); a configured value requires a present token value in the same
/// [`same_bucket`] bucket. Both operands are SOL and the width is SOL.
fn bucket_axis(fp_sol: Option<f64>, tf_sol: Option<f64>, width: f64) -> bool {
    match fp_sol {
        None => true,
        Some(target) => tf_sol.is_some_and(|v| same_bucket(v, target, width)),
    }
}

fn lamports_to_sol(lamports: i64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL_F64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp_id(n: u128) -> FingerprintId {
        FingerprintId(Uuid::from_u128(n))
    }

    /// An all-`None` fingerprint at the default 0.1 SOL width; tests set the axes
    /// they exercise.
    fn blank_fp() -> Fingerprint {
        Fingerprint {
            id: fp_id(1),
            cu_limit: None,
            cu_price: None,
            ix_labels: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: 0.1,
            metric_config: serde_json::json!({}),
        }
    }

    fn blank_tf() -> TokenFingerprint {
        TokenFingerprint::default()
    }

    const SOL: f64 = LAMPORTS_PER_SOL_F64;

    #[test]
    fn no_criteria_never_matches() {
        // An all-None fingerprint matches nothing, at either phase.
        let fp = blank_fp();
        let tf = blank_tf();
        assert!(!matches(&fp, &tf));
        assert!(!matches_phase(&fp, &tf, MatchPhase::Instant));
        assert!(match_all(std::slice::from_ref(&fp), &tf, MatchPhase::Full).is_empty());
    }

    #[test]
    fn exact_cu_axes() {
        let mut fp = blank_fp();
        fp.cu_limit = Some(200_000);
        fp.cu_price = Some(1_000);
        let mut tf = blank_tf();
        tf.cu_limit = Some(200_000);
        tf.cu_price = Some(1_000);
        assert!(matches(&fp, &tf));
        // Wrong value → reject.
        tf.cu_price = Some(999);
        assert!(!matches(&fp, &tf));
        // Missing token value on a configured exact axis → reject.
        tf.cu_price = None;
        assert!(!matches(&fp, &tf));
    }

    #[test]
    fn ix_labels_exact_ordered() {
        let mut fp = blank_fp();
        fp.ix_labels = Some(vec!["A".into(), "B".into(), "C".into()]);
        let with = |labels: &[&str]| {
            let mut tf = blank_tf();
            tf.ix_labels = labels.iter().map(|s| s.to_string()).collect();
            tf
        };
        assert!(matches(&fp, &with(&["A", "B", "C"])));
        assert!(!matches(&fp, &with(&["A", "C", "B"])), "wrong order");
        assert!(!matches(&fp, &with(&["A", "B"])), "subset");
        assert!(!matches(&fp, &with(&["A", "B", "C", "D"])), "superset");
        assert!(!matches(&fp, &with(&[])), "empty token labels");
    }

    #[test]
    fn empty_ix_labels_is_inert_not_a_criterion() {
        // Some(empty) ix_labels is treated as not-configured (mirrors legacy), so
        // it does not count toward the ≥1-criterion guard.
        let mut fp = blank_fp();
        fp.ix_labels = Some(vec![]);
        assert!(!fp.has_any_criterion());
        assert!(!matches(&fp, &blank_tf()));
        // With a second real axis, the empty labels neither help nor block.
        fp.cu_limit = Some(5);
        let mut tf = blank_tf();
        tf.cu_limit = Some(5);
        assert!(matches(&fp, &tf));
    }

    #[test]
    fn init_buy_bucket_membership() {
        // width 0.1 → target 1.0 SOL defines bucket [1.0, 1.1). 1.05 shares it; 1.2 doesn't.
        let mut fp = blank_fp();
        fp.init_buy_lamports = Some(1_000_000_000); // 1.0 SOL
        let near = {
            let mut tf = blank_tf();
            tf.initial_buy_sol = Some(1.05);
            tf
        };
        let far = {
            let mut tf = blank_tf();
            tf.initial_buy_sol = Some(1.2);
            tf
        };
        assert!(matches(&fp, &near));
        assert!(!matches(&fp, &far));
        // Missing token value on a configured bucket axis → reject.
        assert!(!matches(&fp, &blank_tf()));
    }

    #[test]
    fn bucket_edge_at_width_boundary() {
        // On-edge lands in the UPPER bucket (matches grouping::same_bucket): target
        // 0.3 SOL and token 0.35 share [0.3, 0.4); 0.29 does not.
        let mut fp = blank_fp();
        fp.init_buy_lamports = Some((0.3 * SOL) as i64);
        let tf_of = |v: f64| {
            let mut tf = blank_tf();
            tf.initial_buy_sol = Some(v);
            tf
        };
        assert!(matches(&fp, &tf_of(0.35)));
        assert!(!matches(&fp, &tf_of(0.29)));
    }

    #[test]
    fn max_cost_and_spendable_convert_lamports() {
        // Both fingerprint and token store these axes as lamports; the match is in
        // SOL space at the row width.
        let mut fp = blank_fp();
        fp.max_cost_lamports = Some(2_000_000_000); // 2.0 SOL
        fp.spendable_lamports_in = Some(500_000_000); // 0.5 SOL
        let mut tf = blank_tf();
        tf.max_cost_lamports = Some(2_050_000_000); // 2.05 SOL ∈ [2.0, 2.1)
        tf.spendable_lamports_in = Some(540_000_000); // 0.54 SOL ∈ [0.5, 0.6)
        assert!(matches(&fp, &tf));
        tf.spendable_lamports_in = Some(610_000_000); // 0.61 SOL → [0.6,0.7)
        assert!(!matches(&fp, &tf));
    }

    #[test]
    fn first_slot_two_phase_resolution() {
        // A fingerprint with only a first-slot axis: at Instant it passes (nothing
        // instant to violate) but is pending; at Full it needs the settled value.
        let mut fp = blank_fp();
        fp.first_slot_buy_lamports = Some(2_000_000_000); // 2.0 SOL
        assert!(fp.has_first_slot_criteria());
        assert!(!fp.has_instant_criterion());

        // Instant: no first-slot value yet, but instant axes are vacuously satisfied.
        let pending = blank_tf(); // first_slot_buy_sol = None
        assert!(matches_phase(&fp, &pending, MatchPhase::Instant));
        // Full with the value still unknown → reject (configured axis unsatisfied).
        assert!(!matches_phase(&fp, &pending, MatchPhase::Full));

        // Once settled in-bucket, Full matches.
        let settled = {
            let mut tf = blank_tf();
            tf.first_slot_buy_sol = Some(2.05);
            tf
        };
        assert!(matches_phase(&fp, &settled, MatchPhase::Full));
        // Out-of-bucket settled value → reject.
        let settled_far = {
            let mut tf = blank_tf();
            tf.first_slot_buy_sol = Some(3.0);
            tf
        };
        assert!(!matches_phase(&fp, &settled_far, MatchPhase::Full));
    }

    #[test]
    fn instant_axis_still_judged_at_instant_phase() {
        // A fingerprint mixing instant + first-slot axes: a wrong instant axis is
        // rejected even at Instant phase (we don't wait to reject).
        let mut fp = blank_fp();
        fp.cu_limit = Some(200_000);
        fp.first_slot_sell_lamports = Some(1_000_000_000);
        let mut tf = blank_tf();
        tf.cu_limit = Some(199_999); // wrong
        assert!(!matches_phase(&fp, &tf, MatchPhase::Instant));
        // Right instant axis, first-slot deferred → pending pass at Instant.
        tf.cu_limit = Some(200_000);
        assert!(matches_phase(&fp, &tf, MatchPhase::Instant));
    }

    #[test]
    fn multi_match_returns_all_hits_in_order() {
        // Three fingerprints; a token matches the first and third, not the second.
        let mut a = blank_fp();
        a.id = fp_id(10);
        a.cu_limit = Some(200_000);
        let mut b = blank_fp();
        b.id = fp_id(20);
        b.cu_limit = Some(999_999); // won't match
        let mut c = blank_fp();
        c.id = fp_id(30);
        c.init_buy_lamports = Some(1_000_000_000); // 1.0 SOL bucket

        let mut tf = blank_tf();
        tf.cu_limit = Some(200_000);
        tf.initial_buy_sol = Some(1.05);

        let hits = match_all(&[a.clone(), b, c.clone()], &tf, MatchPhase::Full);
        assert_eq!(hits.as_slice(), &[a.id, c.id]);
    }

    #[test]
    fn different_widths_can_both_match_same_token() {
        // A coarse fingerprint (width 1.0) and a fine one (0.1) can both claim the
        // same token — each judges its own axis at its own width.
        let mut coarse = blank_fp();
        coarse.id = fp_id(1);
        coarse.bucket_size_amount = 1.0;
        coarse.init_buy_lamports = Some((1.0 * SOL) as i64); // [1,2)
        let mut fine = blank_fp();
        fine.id = fp_id(2);
        fine.bucket_size_amount = 0.1;
        fine.init_buy_lamports = Some((1.5 * SOL) as i64); // [1.5,1.6)

        let mut tf = blank_tf();
        tf.initial_buy_sol = Some(1.55);
        let hits = match_all(&[coarse.clone(), fine.clone()], &tf, MatchPhase::Full);
        assert_eq!(hits.as_slice(), &[coarse.id, fine.id]);
    }
}
