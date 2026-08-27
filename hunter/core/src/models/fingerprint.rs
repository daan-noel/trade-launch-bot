//! `Fingerprint` — a token-creation shape shared by many strategy rules. Backs
//! the `fingerprints` table (0004 redesign schema).
//!
//! Matching semantics (implemented in `strategies::fingerprint`):
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
use crate::grouping::{MIN_BUCKET_WIDTH_SOL, SOL_BUCKET_WIDTH};

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
    /// SOL width every bucket-matched axis uses, or `NULL` to match those axes on
    /// their **exact** lamports amount (`hunter_engine::grouping::SolPrecision`).
    ///
    /// `NULL` — not `0` — is how "not bucketed" is spelled: a width is a *measured*
    /// quantity, `floor(v/0)` is a division by zero, and this value feeds the live
    /// entry gate. Read it through `hunter_engine::fingerprint::Fingerprint::precision`,
    /// never directly. Enforced by the `fingerprints_bucket_size_amount_positive`
    /// CHECK in `0001_init.sql` (`NULL OR (>= 1e-6 AND <= 1e6)`, on a nullable
    /// column) and [`Fingerprint::validate`].
    pub bucket_size_amount: Option<f64>,
    /// Exact ordered instruction-label sequence of the creation tx.
    pub ix_labels: Option<Vec<String>>,
    /// Match EVERY token, ignoring every other axis.
    ///
    /// A rule always needs a fingerprint, but not every rule is about a creation
    /// shape - one that decides purely on what the tape is doing has none to name.
    /// Leaving all axes `NULL` means *match nothing* (the matcher refuses a
    /// criterion-less row on purpose), so "every token" has to be said out loud or
    /// it is indistinguishable from a half-filled form.
    #[serde(default)]
    pub wildcard: bool,
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
            wildcard: body.get("wildcard").and_then(|v| v.as_bool()).unwrap_or(false),
            // Present-and-numeric ⇒ a bucket width; explicit `null` ⇒ exact match;
            // **absent ⇒ the 0.1 default**, so an older client that never heard of
            // exact mode can't silently create exact-matching fingerprints. Only a
            // deliberate `null` opts in.
            bucket_size_amount: match body.get("bucket_size_amount") {
                None => Some(0.1),
                Some(serde_json::Value::Null) => None,
                Some(v) => Some(tidy_sol_decimal(v.as_f64().unwrap_or(0.1))),
            },
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

    /// How this row's SOL axes match — delegated to the engine's
    /// `SolPrecision::from_width` so the DB model and the match-time copy can
    /// never read the same stored width differently.
    pub fn precision(&self) -> hunter_engine::grouping::SolPrecision {
        hunter_engine::grouping::SolPrecision::from_width(self.bucket_size_amount)
    }

    /// The ONE write-edge gate for a persisted fingerprint — called by the live +
    /// lab create/update handlers (for a 400) and again by `FingerprintRepo`
    /// insert/update (backstop for non-HTTP writers like sweep promotion). The DB
    /// `fingerprints_bucket_size_amount_positive` CHECK (`0001_init.sql`) is the
    /// last line of defence.
    ///
    /// * **At least one match criterion.** An all-`None` row hits the matcher's
    ///   own never-match-everything guard, so it silently matches *nothing* and
    ///   quietly kills every rule pointing at it.
    /// * **`bucket_size_amount` is `NULL`, or finite and >= [`MIN_BUCKET_WIDTH_SOL`].**
    ///   `NULL` selects exact-lamports matching. A *present* width is still divided
    ///   by **raw** (`grouping::bucket_index`), so a `0` sends every positive amount
    ///   to the same saturated bucket index and a configured SOL axis stops
    ///   discriminating — the fingerprint arms on any non-zero value. Zero remains an
    ///   *invalid* width, never a second spelling of "exact": `NULL` is the one
    ///   spelling, so the two readers of this field can't disagree. (Distinct again
    ///   from a `None` **axis**, which means "not part of identity" — a fingerprint
    ///   axis of `0` SOL is a real bucket, `[0, width)`.)
    pub fn validate(&self) -> Result<(), String> {
        if !self.has_any_criterion() {
            return Err("fingerprint must configure at least one match criterion".into());
        }
        if let Some(w) = self.bucket_size_amount {
            if !w.is_finite() || w < MIN_BUCKET_WIDTH_SOL {
                return Err(format!(
                    "bucket_size_amount must be null (exact match) or a finite SOL width \
                     >= {MIN_BUCKET_WIDTH_SOL} (got {w})"
                ));
            }
        }
        Ok(())
    }

    /// Compact label from the match axes — the one auto-name every create path
    /// uses (sweep promote, creation-stats, flow-discovery bind, blank form).
    /// Identity stays on the axes; this is a picker/log handle. Chip tokens, ix
    /// first, default `0.1` bucket omitted. Detail:
    /// `docs/plans/strategies/fingerprint-auto-name.md`.
    pub fn auto_name(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(labels) = configured_labels(self.ix_labels.as_deref()) {
            parts.push(ix_labels_count_tail(labels));
        }
        if let Some(n) = self.cu_limit {
            parts.push(format!("cu_limit={}", format_compact_int(n)));
        }
        if let Some(n) = self.cu_price {
            parts.push(format!("cu_price={}", format_compact_int(n)));
        }
        push_sol_part(&mut parts, "init", self.init_buy_lamports);
        push_sol_part(&mut parts, "max", self.max_cost_lamports);
        push_sol_part(&mut parts, "spend", self.spendable_lamports_in);
        push_sol_part(&mut parts, "fs_buy", self.first_slot_buy_lamports);
        push_sol_part(&mut parts, "fs_sell", self.first_slot_sell_lamports);
        match self.bucket_size_amount {
            None => parts.push("bkt=exact".into()),
            Some(w) => {
                let width = tidy_sol_decimal(w);
                if width != SOL_BUCKET_WIDTH {
                    parts.push(format!("bkt={}", format_decimal_trim(width, 4)));
                }
            }
        }
        if parts.is_empty() {
            "ALL".into()
        } else {
            parts.join(" · ")
        }
    }

    /// True when `name` is blank or a retired auto-label (`sweep {id} · group N`,
    /// `c · …` / `f · …` / `s · …`, `flow-discovery bind`). Nicknames stay.
    pub fn has_legacy_auto_name(&self) -> bool {
        is_legacy_auto_name(&self.name)
    }

    /// Replace a blank / legacy auto-label with [`Self::auto_name`]. A nickname
    /// is left untouched.
    pub fn ensure_auto_name(&mut self) {
        if self.has_legacy_auto_name() {
            self.name = self.auto_name();
        }
    }
}

/// Retired auto-name shapes. Mirrored in the TS `isLegacyAutoName` helper —
/// the two lists stay equal (guarded by the golden-string tests on `auto_name`).
pub fn is_legacy_auto_name(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    if n.eq_ignore_ascii_case("flow-discovery bind") {
        return true;
    }
    if let Some(rest) = n.strip_prefix("sweep ") {
        if rest.contains(" · group ") {
            return true;
        }
    }
    n.starts_with("c · ") || n.starts_with("f · ") || n.starts_with("s · ")
}

fn push_sol_part(parts: &mut Vec<String>, label: &str, lamports: Option<i64>) {
    if let Some(l) = lamports {
        parts.push(format!("{label}={}", format_decimal_trim(lamports_to_sol(l), 4)));
    }
}

/// `"Pump.Fun: Buy"` → `"Buy"`. Split on the last `": "` so a program name
/// containing a colon still resolves. Mirrors TS `ixLabelAction`.
fn ix_label_action(label: &str) -> &str {
    match label.rfind(": ") {
        Some(i) => label[i + 2..].trim(),
        None => label.trim(),
    }
}

/// `"3ix:Buy"` — count plus trailing action. Mirrors TS `ixLabelsCountTail`.
fn ix_labels_count_tail(labels: &[String]) -> String {
    let n = labels.len();
    let tail = labels.last().map(|s| ix_label_action(s)).unwrap_or("");
    if tail.is_empty() {
        format!("{n}ix")
    } else {
        format!("{n}ix:{tail}")
    }
}

/// `toFixed(decimals)` then strip trailing zeros — mirrors TS `formatDecimalTrim`.
fn format_decimal_trim(value: f64, decimals: usize) -> String {
    let s = format!("{value:.decimals$}");
    if !s.contains('.') {
        return s;
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        s
    } else {
        trimmed.to_string()
    }
}

/// Chip-aligned compact int (`80K`, `200K`). Mirrors TS `formatCompact(n, 1)`.
fn format_compact_int(n: i64) -> String {
    let abs = n.unsigned_abs();
    let sign = if n < 0 { "-" } else { "" };
    if abs >= 1_000_000_000 {
        format!("{sign}{}G", format_decimal_trim(abs as f64 / 1_000_000_000.0, 1))
    } else if abs >= 1_000_000 {
        format!("{sign}{}M", format_decimal_trim(abs as f64 / 1_000_000.0, 1))
    } else if abs >= 1_000 {
        format!("{sign}{}K", format_decimal_trim(abs as f64 / 1_000.0, 1))
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp_with(bucket_size_amount: Option<f64>) -> Fingerprint {
        let now = Utc::now();
        Fingerprint {
            wildcard: false,
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
            let err = fp_with(Some(bad)).validate().unwrap_err();
            assert!(err.contains("bucket_size_amount"), "width {bad} not rejected: {err}");
        }
        for ok in [MIN_BUCKET_WIDTH_SOL, 0.1, 1.0, 5.0] {
            assert!(fp_with(Some(ok)).validate().is_ok(), "width {ok} wrongly rejected");
        }
    }

    #[test]
    fn a_zero_sol_axis_is_a_criterion_not_an_unset_field() {
        // 0 lamports is a real axis value (bucket `[0, width)`), distinct from
        // `None`. It must survive the criterion count, unlike the caps elsewhere
        // in the codebase where 0 means "off".
        let mut fp = fp_with(Some(1.0));
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
            let mut fp = fp_with(Some(0.1));
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
        let mut fp = fp_with(Some(0.1));
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

    fn bare() -> Fingerprint {
        let mut fp = fp_with(Some(0.1));
        fp.cu_limit = None;
        fp
    }

    /// Golden strings — keep byte-equal with the TS `fingerprintAutoName` tests.
    #[test]
    fn auto_name_golden() {
        let mut fp = bare();
        fp.ix_labels = Some(vec![
            "Pump.Fun: Create_v2".into(),
            "Associated Token: CreateIdempotent".into(),
            "Pump.Fun: Buy".into(),
        ]);
        fp.max_cost_lamports = Some(1_000_000_000);
        fp.bucket_size_amount = Some(1.0);
        assert_eq!(fp.auto_name(), "3ix:Buy · max=1 · bkt=1");

        let mut fp = bare();
        fp.ix_labels = Some(vec![
            "Pump.Fun: Create_v2".into(),
            "Associated Token: CreateIdempotent".into(),
            "Pump.Fun: Buy".into(),
        ]);
        fp.first_slot_buy_lamports = Some(19_500_000_000);
        fp.first_slot_sell_lamports = Some(0);
        fp.max_cost_lamports = Some(0);
        fp.bucket_size_amount = Some(0.5);
        assert_eq!(fp.auto_name(), "3ix:Buy · max=0 · fs_buy=19.5 · fs_sell=0 · bkt=0.5");

        let mut buy = bare();
        buy.cu_limit = Some(80_000);
        buy.ix_labels = Some(vec![
            "Pump.Fun: Create_v2".into(),
            "Associated Token: Create".into(),
            "Pump.Fun: Buy".into(),
        ]);
        let mut exact = buy.clone();
        exact.ix_labels = Some(vec![
            "Pump.Fun: Create_v2".into(),
            "Associated Token: Create".into(),
            "Pump.Fun: BuyExactSolIn".into(),
        ]);
        assert_eq!(buy.auto_name(), "3ix:Buy · cu_limit=80K");
        assert_eq!(exact.auto_name(), "3ix:BuyExactSolIn · cu_limit=80K");
        assert_ne!(buy.auto_name(), exact.auto_name());

        let mut fp = bare();
        fp.first_slot_buy_lamports = Some(19_500_000_000);
        fp.ix_labels = Some(vec!["A".into(), "B".into()]);
        assert_eq!(fp.auto_name(), "2ix:B · fs_buy=19.5");

        let mut fp = bare();
        fp.cu_limit = Some(200_000);
        assert_eq!(fp.auto_name(), "cu_limit=200K");

        assert_eq!(bare().auto_name(), "ALL");

        let mut fp = bare();
        fp.max_cost_lamports = Some(1_000_000_000);
        fp.ix_labels = Some(vec!["Pump.Fun: Buy".into()]);
        fp.bucket_size_amount = None;
        assert_eq!(fp.auto_name(), "1ix:Buy · max=1 · bkt=exact");
    }

    #[test]
    fn legacy_auto_name_detects_retired_shapes_only() {
        assert!(is_legacy_auto_name(""));
        assert!(is_legacy_auto_name("  "));
        assert!(is_legacy_auto_name("sweep 0f53d622 · group 12"));
        assert!(is_legacy_auto_name("c · max1 · b1"));
        assert!(is_legacy_auto_name("f · cu200000"));
        assert!(is_legacy_auto_name("s · ALL"));
        assert!(is_legacy_auto_name("flow-discovery bind"));
        assert!(!is_legacy_auto_name("3ix:Buy · max=1 · bkt=1"));
        assert!(!is_legacy_auto_name("max-buy launcher"));
    }

    #[test]
    fn ensure_auto_name_replaces_legacy_keeps_nickname() {
        let mut fp = bare();
        fp.max_cost_lamports = Some(1_000_000_000);
        fp.bucket_size_amount = Some(1.0);
        fp.name = "sweep 0f53d622 · group 12".into();
        fp.ensure_auto_name();
        assert_eq!(fp.name, "max=1 · bkt=1");

        fp.name = "max-buy launcher".into();
        fp.ensure_auto_name();
        assert_eq!(fp.name, "max-buy launcher");
    }
}
