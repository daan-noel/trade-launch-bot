//! `Fingerprint` — a token-creation shape shared by many strategy rules. Backs
//! the `fingerprints` table (0004 redesign schema).
//!
//! Matching semantics (implemented in `strategies::fingerprint`, Phase 2):
//! * **Exact** fields: `cu_limit`, `cu_price`, `ix_labels` (exact ordered
//!   sequence).
//! * **Bucket-matched** fields (via this row's own `bucket_size_amount`, SSOT
//!   [`crate::grouping::same_bucket`]): the five lamports axes.
//! * A `None` field is **not part of the fingerprint's identity** (ignored by
//!   the matcher). A token can match multiple fingerprints.
//!
//! Unit convention: lamports (`i64`) at rest, human SOL (`f64`) via the `*_sol`
//! accessors — conversion through the one shared `config::constants` pair.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use hunter_engine::fingerprint::configured_labels;

use crate::config::constants::{lamports_to_sol, tidy_sol_decimal};
use crate::grouping::MIN_BUCKET_WIDTH_SOL;

/// Read an optional integer field from an HTTP JSON body (accepts a JSON number
/// or a numeric string). Shared SSOT for the generic-engine CRUD parse paths.
pub fn opt_i64(body: &serde_json::Value, key: &str) -> Option<i64> {
    body.get(key).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

/// A `fingerprints` row. See module docs for matching semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub id: Uuid,
    /// Human-facing label.
    pub name: String,
    /// Exact-match compute-unit limit of the creation tx.
    pub cu_limit: Option<i64>,
    /// Exact-match compute-unit price of the creation tx.
    pub cu_price: Option<i64>,
    /// Creator's initial dev-buy size (bucket-matched).
    pub init_buy_lamports: Option<i64>,
    /// `max_sol_cost` arg of the initial buy instruction (bucket-matched).
    pub max_cost_lamports: Option<i64>,
    /// Spendable SOL the creator wallet held going in (bucket-matched).
    pub spendable_lamports_in: Option<i64>,
    /// Sum of buy SOL in the creation slot (bucket-matched; only settled after
    /// the creation slot closes — deferred first-slot gate).
    pub first_slot_buy_lamports: Option<i64>,
    /// Sum of sell SOL in the creation slot (bucket-matched; deferred as above).
    pub first_slot_sell_lamports: Option<i64>,
    /// SOL width of the bucket every bucket-matched axis uses.
    pub bucket_size_amount: f64,
    /// Exact ordered instruction-label sequence of the creation tx.
    pub ix_labels: Option<Vec<String>>,
    /// Per-metric-group fingerprint-side config (e.g. `m_flow_split.volume_ix_patterns`).
    /// **Not** part of match identity — `find_or_create` ignores it.
    #[serde(default = "default_metric_config")]
    pub metric_config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

    /// Parse a fingerprint from a raw HTTP JSON body — the SSOT for the wire
    /// shape, shared by the live + lab CRUD handlers. Amounts are lamports on
    /// the wire; `id` and the timestamps are caller-supplied (not read from the
    /// body). Lenient: absent/unparseable numeric fields are `None`.
    pub fn from_json(body: &serde_json::Value, id: Uuid, now: DateTime<Utc>) -> Self {
        Fingerprint {
            id,
            name: body.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            cu_limit: opt_i64(body, "cu_limit"),
            cu_price: opt_i64(body, "cu_price"),
            init_buy_lamports: opt_i64(body, "init_buy_lamports"),
            max_cost_lamports: opt_i64(body, "max_cost_lamports"),
            spendable_lamports_in: opt_i64(body, "spendable_lamports_in"),
            first_slot_buy_lamports: opt_i64(body, "first_slot_buy_lamports"),
            first_slot_sell_lamports: opt_i64(body, "first_slot_sell_lamports"),
            bucket_size_amount: tidy_sol_decimal(
                body.get("bucket_size_amount")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.1),
            ),
            // Normalize the empty label list to `None` at the boundary so the
            // ambiguous "set but empty" state never reaches storage or a matcher
            // — `Some([])` and `None` mean the same thing, so only one of them
            // gets to exist (see `configured_labels`).
            ix_labels: body
                .get("ix_labels")
                .and_then(|v| v.as_array())
                .map(|a| -> Vec<String> {
                    a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
                })
                .filter(|v| !v.is_empty()),
            metric_config: body
                .get("metric_config")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(default_metric_config),
            created_at: now,
            updated_at: now,
        }
    }

    /// Whether any matchable criterion is configured. The matcher requires at
    /// least one so an all-`None` fingerprint can never match everything.
    pub fn has_any_criterion(&self) -> bool {
        self.cu_limit.is_some()
            || self.cu_price.is_some()
            || self.init_buy_lamports.is_some()
            || self.max_cost_lamports.is_some()
            || self.spendable_lamports_in.is_some()
            || self.first_slot_buy_lamports.is_some()
            || self.first_slot_sell_lamports.is_some()
            // `Some([])` is NOT a criterion — the engine matcher's SSOT decides,
            // never a bare `is_some()`. See `configured_labels`.
            || configured_labels(self.ix_labels.as_deref()).is_some()
    }

    /// The ONE write-edge gate for a persisted fingerprint — called by the live +
    /// lab create/update handlers (for a 400) and again by `FingerprintRepo`
    /// insert/update (backstop for non-HTTP writers like sweep promotion). The DB
    /// `CHECK` added in `0014_fingerprint_bucket_width_positive.sql` is the last
    /// line of defence.
    ///
    /// * **At least one match criterion.** An all-`None` row hits the matcher's
    ///   own never-match-everything guard, so it silently matches *nothing* and
    ///   quietly kills every rule pointing at it.
    /// * **`bucket_size_amount` finite and >= [`MIN_BUCKET_WIDTH_SOL`].** The
    ///   matcher divides by this width **raw** (`grouping::bucket_index`), so a
    ///   `0` width sends every positive amount to the same saturated bucket index
    ///   and a configured SOL axis stops discriminating — the fingerprint arms on
    ///   any non-zero value. Zero is an *invalid* width here, never a "not set"
    ///   sentinel: a fingerprint axis of `0` SOL is a real bucket (`[0, width)`),
    ///   and `None` is the only way to say "axis not part of identity".
    pub fn validate(&self) -> Result<(), String> {
        if !self.has_any_criterion() {
            return Err("fingerprint must configure at least one match criterion".into());
        }
        let w = self.bucket_size_amount;
        if !w.is_finite() || w < MIN_BUCKET_WIDTH_SOL {
            return Err(format!(
                "bucket_size_amount must be a finite SOL width >= {MIN_BUCKET_WIDTH_SOL} (got {w})"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp_with(bucket_size_amount: f64) -> Fingerprint {
        let now = Utc::now();
        Fingerprint {
            id: Uuid::nil(),
            name: "t".into(),
            cu_limit: Some(200_000),
            cu_price: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount,
            ix_labels: None,
            metric_config: default_metric_config(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn rejects_non_positive_or_non_finite_bucket_width() {
        // A 0 width makes `bucket_index` saturate every positive value to the same
        // index, so a configured SOL axis would match any non-zero amount.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, MIN_BUCKET_WIDTH_SOL / 2.0] {
            let err = fp_with(bad).validate().unwrap_err();
            assert!(err.contains("bucket_size_amount"), "width {bad} not rejected: {err}");
        }
        for ok in [MIN_BUCKET_WIDTH_SOL, 0.1, 1.0, 5.0] {
            assert!(fp_with(ok).validate().is_ok(), "width {ok} wrongly rejected");
        }
    }

    #[test]
    fn a_zero_sol_axis_is_a_criterion_not_an_unset_field() {
        // 0 lamports is a real axis value (bucket `[0, width)`), distinct from
        // `None`. It must survive the criterion count, unlike the caps elsewhere
        // in the codebase where 0 means "off".
        let mut fp = fp_with(1.0);
        fp.cu_limit = None;
        assert!(!fp.has_any_criterion(), "all-None must not count");
        fp.spendable_lamports_in = Some(0);
        assert!(fp.has_any_criterion(), "a 0-lamport axis must count as configured");
        assert!(fp.validate().is_ok());
    }

    /// SSOT guard: this model's `has_any_criterion` and the engine matcher's must
    /// return the SAME verdict for every fingerprint, because they gate opposite
    /// failure modes — the matcher turns "no criteria" into *matches nothing*, the
    /// creation-stats SQL mirror turns it into *matches everything in the window*.
    /// A disagreement means the live engine and the dashboard describe different
    /// token sets for the same saved row.
    #[test]
    fn has_any_criterion_agrees_with_engine() {
        use crate::strategies::fingerprint_axes::fp_to_engine;

        // An axis-free fingerprint; each case sets exactly the axis it exercises.
        let bare = || {
            let mut fp = fp_with(0.1);
            fp.cu_limit = None;
            fp
        };
        let agree = |name: &str, fp: &Fingerprint| {
            assert_eq!(
                fp.has_any_criterion(),
                fp_to_engine(fp).has_any_criterion(),
                "model and engine disagree on `{name}`",
            );
        };

        agree("all-none", &bare());
        agree("cu_limit", &Fingerprint { cu_limit: Some(200_000), ..bare() });
        agree("zero-axis", &Fingerprint { spendable_lamports_in: Some(0), ..bare() });
        agree("first-slot", &Fingerprint { first_slot_buy_lamports: Some(1), ..bare() });
        // The regression: `Some([])` is a second spelling of "not set". The model
        // used to count it as a criterion while the engine did not.
        agree("empty-labels", &Fingerprint { ix_labels: Some(vec![]), ..bare() });
        agree("real-labels", &Fingerprint { ix_labels: Some(vec!["A".into()]), ..bare() });
    }

    #[test]
    fn empty_ix_labels_is_not_a_criterion() {
        // A fingerprint whose ONLY axis is an empty label list configures nothing:
        // it must be rejected at the write edge rather than saved to match nothing
        // in the engine and everything on the dashboard.
        let mut fp = fp_with(0.1);
        fp.cu_limit = None;
        fp.ix_labels = Some(vec![]);
        assert!(!fp.has_any_criterion());
        assert!(fp.validate().unwrap_err().contains("match criterion"));

        // Alongside a real axis it is simply inert, never a match constraint.
        fp.cu_limit = Some(5);
        assert!(fp.validate().is_ok());
    }

    #[test]
    fn from_json_normalizes_empty_ix_labels_to_none() {
        // The ambiguous "set but empty" state must not survive the wire parse, so
        // it can never reach storage and become a second reader's problem.
        let body = serde_json::json!({ "cu_limit": 1, "ix_labels": [] });
        let fp = Fingerprint::from_json(&body, Uuid::nil(), Utc::now());
        assert_eq!(fp.ix_labels, None);

        let body = serde_json::json!({ "cu_limit": 1, "ix_labels": ["A", "B"] });
        let fp = Fingerprint::from_json(&body, Uuid::nil(), Utc::now());
        assert_eq!(fp.ix_labels.as_deref(), Some(["A".to_string(), "B".to_string()].as_slice()));
    }

    #[test]
    fn from_json_keeps_an_explicit_zero_axis() {
        // The wire parse must not fold an explicit 0 into `None` — `opt_i64`
        // distinguishes absent from zero.
        let body = serde_json::json!({ "spendable_lamports_in": 0, "cu_limit": null });
        let fp = Fingerprint::from_json(&body, Uuid::nil(), Utc::now());
        assert_eq!(fp.spendable_lamports_in, Some(0));
        assert_eq!(fp.cu_limit, None);
    }
}
