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
use crate::grouping::{decimals_for, MIN_BUCKET_WIDTH_SOL, SOL_BUCKET_WIDTH};

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
        let mut fp = Fingerprint {
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
        };
        // Collapse an inert width to the one spelling of "nothing is bucketed here",
        // exactly as the empty label list above collapses to `None`. A client that
        // posts the 0.1 default alongside a labels-only fingerprint otherwise stores
        // a second identity for a match the engine makes identically.
        fp.bucket_size_amount = fp.effective_bucket_size_amount();
        fp
    }

    /// Whether any matchable criterion is configured. The matcher requires at
    /// least one so an all-`None` fingerprint can never match everything.
    ///
    /// [`Self::wildcard`] IS a criterion — the explicitly-spelled "every token"
    /// one — and must be counted here exactly as the engine matcher counts it
    /// (`hunter_engine::fingerprint::Fingerprint::has_any_criterion`). Omitting it
    /// on this side makes a wildcard row unsaveable through [`Self::validate`]
    /// while the matcher would have armed it on everything: the two readers of one
    /// row disagreeing about whether it configures anything at all.
    pub fn has_any_criterion(&self) -> bool {
        self.wildcard || self.has_axis_criterion()
    }

    /// Whether any **axis** is configured, ignoring [`Self::wildcard`]. Separate
    /// from [`Self::has_any_criterion`] because the two ask opposite questions of
    /// a wildcard row: it always *has* a criterion, and it must never carry an axis.
    pub fn has_axis_criterion(&self) -> bool {
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

    /// Whether any **bucket-matched SOL** axis is configured. The one reader of
    /// "is [`Self::bucket_size_amount`] load-bearing on this row" — the width only
    /// ever reaches the matcher through one of these five axes.
    pub fn has_sol_axis(&self) -> bool {
        self.init_buy_lamports.is_some()
            || self.max_cost_lamports.is_some()
            || self.spendable_lamports_in.is_some()
            || self.first_slot_buy_lamports.is_some()
            || self.first_slot_sell_lamports.is_some()
    }

    /// The width **as the matcher sees it**: `None` unless a SOL axis is configured.
    ///
    /// With no SOL axis there is nothing to bucket, so a stored width is inert — it
    /// changes no match. Left uncanonicalised it becomes a second spelling of the
    /// same fingerprint, and both readers of the field get it wrong in their own
    /// way: `FingerprintRepo::IDENTITY_WHERE` keys on the raw width, so an inert
    /// one forks identity and `find_or_create` mints a duplicate instead of reusing
    /// the row; [`Self::auto_name`] prints it, so the same match carries two names.
    /// This is the same "one spelling per state" collapse `configured_labels` makes
    /// for `Some([])` and [`Self::auto_name`] already makes for a wildcard.
    ///
    /// Every write edge stores THIS, never the raw field ([`Self::from_json`] at the
    /// HTTP boundary, `FingerprintRepo` insert/update/find_or_create for non-HTTP
    /// writers like sweep promotion), and the
    /// `fingerprints_bucket_width_needs_a_sol_axis` CHECK (`0006`) is the backstop.
    pub fn effective_bucket_size_amount(&self) -> Option<f64> {
        if self.has_sol_axis() {
            self.bucket_size_amount
        } else {
            None
        }
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
    /// * **A wildcard row carries no axis.** `wildcard` already answers the match
    ///   for every token, so an axis alongside it is a contradiction the matcher
    ///   resolves silently in favour of the wildcard — the operator would read the
    ///   axes on the row and expect them to narrow it. Mirrors the
    ///   `fingerprints_wildcard_excludes_axes` CHECK (`0005`), rejected here so the
    ///   write edge answers `400` instead of a DB error.
    pub fn validate(&self) -> Result<(), String> {
        if !self.has_any_criterion() {
            return Err("fingerprint must configure at least one match criterion".into());
        }
        if self.wildcard && self.has_axis_criterion() {
            return Err(
                "a wildcard fingerprint matches every token, so it cannot also carry                  match axes — clear the axes or turn the wildcard off"
                    .into(),
            );
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
        // A wildcard row has no axes to name (the `0005` CHECK guarantees it) and
        // its bucket width is inert, so it names the token set it matches.
        if self.wildcard {
            return WILDCARD_NAME.into();
        }
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
        // The *effective* width, so a row with no SOL axis never names a width that
        // reaches no match — the same reason the wildcard arm above names none. It
        // also keeps the name in step with the stored value every write edge writes.
        //
        // Rendered at `decimals_for(width)`, not a fixed 4: the legal range reaches
        // down to `MIN_BUCKET_WIDTH_SOL` (1e-6), and a fixed 4 trimmed a `1e-5` width
        // to `bkt=0` — a name stating the one width `validate` rejects.
        match self.effective_bucket_size_amount() {
            None if self.has_sol_axis() => parts.push("bkt=exact".into()),
            None => {}
            Some(w) => {
                let width = tidy_sol_decimal(w);
                if width != SOL_BUCKET_WIDTH {
                    parts.push(format!("bkt={}", format_decimal_trim(width, decimals_for(width))));
                }
            }
        }
        if parts.is_empty() {
            WILDCARD_NAME.into()
        } else {
            parts.join(" · ")
        }
    }

    /// True when `name` is blank or a retired auto-label (`sweep {id} · group N`,
    /// `c · …` / `f · …` / `s · …`, `flow-discovery bind`). Nicknames stay.
    pub fn has_legacy_auto_name(&self) -> bool {
        is_legacy_auto_name(&self.name)
    }

    /// True when `name` is an auto-label that no longer says what the axes say —
    /// a retired shape, or a **current-grammar** one that has since drifted from
    /// [`Self::auto_name`].
    ///
    /// The second case is what lets a naming change finish. `auto_name` is a pure
    /// function of the axes, but its output is *stored*, so every edit to it strands
    /// the copies already written — two rows with identical axes then read as two
    /// fingerprints, which is the whole problem the name exists to prevent. Deciding
    /// it by grammar ([`is_generated_auto_name`]) rather than by an ever-growing list
    /// of retired prefixes means the next change to `auto_name` heals itself.
    ///
    /// A nickname is not in the grammar, so it is never touched — it is the only
    /// record of *why* a fingerprint exists, and the axes can always be re-read.
    pub fn has_stale_auto_name(&self) -> bool {
        self.has_legacy_auto_name()
            || (is_generated_auto_name(&self.name) && self.name != self.auto_name())
    }

    /// Replace a blank / stale auto-label with [`Self::auto_name`]. A nickname
    /// is left untouched.
    pub fn ensure_auto_name(&mut self) {
        if self.has_stale_auto_name() {
            self.name = self.auto_name();
        }
    }
}

/// Whether `name` is written in [`Fingerprint::auto_name`]'s own chip grammar:
/// every ` · `-separated part is a chip that function emits. Such a name was
/// generated, never typed, so [`Fingerprint::has_stale_auto_name`] may rewrite it
/// once it stops matching the axes.
///
/// Deliberately strict — an unrecognised part makes the whole name a nickname.
/// The cost of the two mistakes is not symmetric: re-deriving a name it declined
/// to touch is free, while rewriting a real nickname destroys the only record of
/// why that fingerprint was created. Mirrored by the TS `isGeneratedAutoName`.
pub fn is_generated_auto_name(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return false;
    }
    if n == WILDCARD_NAME {
        return true;
    }
    n.split(" · ").all(is_auto_name_chip)
}

/// One chip of the [`is_generated_auto_name`] grammar. Kept beside `auto_name` so
/// a chip added there is added here in the same edit.
fn is_auto_name_chip(part: &str) -> bool {
    // `3ix` / `3ix:BuyExactSolIn` — the count is what makes it a chip and not a
    // word; a nickname prefix like `8dtx` is not `{digits}ix`.
    if let Some((count, tail)) = part.split_once("ix") {
        let tail_ok = tail.is_empty() || tail.strip_prefix(':').is_some_and(|t| !t.is_empty());
        if !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit()) && tail_ok {
            return true;
        }
    }
    let Some((label, value)) = part.split_once('=') else { return false };
    match label {
        // `format_compact_int` — a decimal with an optional K/M/G scale suffix.
        "cu_limit" | "cu_price" => {
            is_decimal(value.strip_suffix(['K', 'M', 'G']).unwrap_or(value))
        }
        // `push_sol_part` — a plain trimmed decimal.
        "init" | "max" | "spend" | "fs_buy" | "fs_sell" => is_decimal(value),
        "bkt" => value == "exact" || is_decimal(value),
        _ => false,
    }
}

/// `format_decimal_trim` output: an optional sign, digits, at most one `.`, and
/// no trailing separator.
fn is_decimal(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    if body.is_empty() {
        return false;
    }
    let mut parts = body.split('.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// Auto-name of a fingerprint with nothing to name from its axes: a `wildcard`
/// row (which matches every token) and — for the criterion-less draft the write
/// edge rejects — the same word, because both describe the same token set. Callers
/// that need "is there anything to name here" compare against this. Mirrored by
/// the TS `WILDCARD_NAME`.
pub const WILDCARD_NAME: &str = "ALL";

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
        // The second instance of the same regression: the engine matcher counts
        // `wildcard` as the explicit "every token" criterion, so this side must too
        // — otherwise `validate` rejects the one row the matcher arms on everything.
        agree("wildcard", &Fingerprint { wildcard: true, ..bare() });
    }

    #[test]
    fn a_wildcard_is_a_criterion_and_saves() {
        let fp = Fingerprint { wildcard: true, ..bare() };
        assert!(fp.has_any_criterion(), "a wildcard configures the every-token criterion");
        assert!(!fp.has_axis_criterion(), "a wildcard is not an axis");
        assert!(fp.validate().is_ok(), "a wildcard row must be saveable: {:?}", fp.validate());
    }

    #[test]
    fn a_wildcard_may_not_carry_axes() {
        // Mirrors the `fingerprints_wildcard_excludes_axes` CHECK: the matcher
        // short-circuits on `wildcard`, so an axis alongside it would be a
        // constraint the operator can read on the row but that never applies.
        let with_axis = Fingerprint { wildcard: true, cu_limit: Some(200_000), ..bare() };
        assert!(with_axis.validate().unwrap_err().contains("wildcard"));

        let with_labels =
            Fingerprint { wildcard: true, ix_labels: Some(vec!["A".into()]), ..bare() };
        assert!(with_labels.validate().unwrap_err().contains("wildcard"));

        // An EMPTY label list is "not set" (`configured_labels`), so it is not an
        // axis and must not trip the guard — the same verdict the CHECK reaches.
        let empty_labels = Fingerprint { wildcard: true, ix_labels: Some(vec![]), ..bare() };
        assert!(empty_labels.validate().is_ok());
    }

    #[test]
    fn a_wildcard_auto_names_all_and_ignores_the_inert_width() {
        // The width never reaches a wildcard match, so it must not reach its name.
        let fp = Fingerprint { wildcard: true, bucket_size_amount: None, ..bare() };
        assert_eq!(fp.auto_name(), WILDCARD_NAME);
        assert_eq!(Fingerprint { wildcard: true, ..bare() }.auto_name(), WILDCARD_NAME);
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

    /// A width with no SOL axis to spend it on changes no match, so it must not
    /// change the name or the identity either — the two ways the same fingerprint
    /// used to end up stored twice under two labels.
    #[test]
    fn an_inert_bucket_width_reaches_neither_the_name_nor_storage() {
        let labels_only = |w: Option<f64>| Fingerprint {
            ix_labels: Some(vec!["Pump.Fun: Create_v2".into(), "Pump.Fun: Buy".into()]),
            bucket_size_amount: w,
            ..bare()
        };
        // Same match at every width, so one name and one stored width.
        for w in [Some(1000.0), Some(0.1), Some(1.0), None] {
            let fp = labels_only(w);
            assert!(!fp.has_sol_axis(), "width {w:?}: no SOL axis to bucket");
            assert_eq!(fp.auto_name(), "2ix:Buy", "width {w:?} leaked into the name");
            assert_eq!(fp.effective_bucket_size_amount(), None, "width {w:?} stored");
        }
        // One SOL axis and the width is load-bearing again — including `exact`.
        let mut fp = labels_only(Some(1000.0));
        fp.max_cost_lamports = Some(1_000_000_000);
        assert!(fp.has_sol_axis());
        assert_eq!(fp.effective_bucket_size_amount(), Some(1000.0));
        assert_eq!(fp.auto_name(), "2ix:Buy · max=1 · bkt=1000");
        fp.bucket_size_amount = None;
        assert_eq!(fp.auto_name(), "2ix:Buy · max=1 · bkt=exact");
    }

    /// The inert width must be gone by the time it is stored, not merely ignored on
    /// read — the identity predicate keys on the column.
    #[test]
    fn from_json_drops_a_width_with_no_sol_axis_to_spend_it_on() {
        // The 0.1 default the form posts alongside a labels-only fingerprint.
        let body = serde_json::json!({ "ix_labels": ["A", "B"] });
        assert_eq!(Fingerprint::from_json(&body, Uuid::nil(), Utc::now()).bucket_size_amount, None);

        let body = serde_json::json!({ "cu_limit": 80_000, "bucket_size_amount": 1000.0 });
        assert_eq!(Fingerprint::from_json(&body, Uuid::nil(), Utc::now()).bucket_size_amount, None);

        // A SOL axis keeps it.
        let body = serde_json::json!({ "max_cost_lamports": 1, "bucket_size_amount": 1000.0 });
        let fp = Fingerprint::from_json(&body, Uuid::nil(), Utc::now());
        assert_eq!(fp.bucket_size_amount, Some(1000.0));
    }

    /// A width is legal down to `MIN_BUCKET_WIDTH_SOL` (1e-6). Rendering it at a
    /// fixed 4 decimals trimmed `1e-5` to `bkt=0` — a name stating the one width
    /// `validate` rejects, on a row whose real width is fine.
    #[test]
    fn auto_name_renders_a_sub_milli_width_instead_of_trimming_it_to_zero() {
        let mut fp = bare();
        fp.max_cost_lamports = Some(270_000_000);
        fp.bucket_size_amount = Some(1e-5);
        assert_eq!(fp.auto_name(), "max=0.27 · bkt=0.00001");
        assert!(fp.validate().is_ok(), "the width itself is legal");

        fp.bucket_size_amount = Some(MIN_BUCKET_WIDTH_SOL);
        assert_eq!(fp.auto_name(), "max=0.27 · bkt=0.000001");
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

    /// The grammar decides "generated, not typed". Getting this wrong in the
    /// permissive direction destroys a nickname, so the real names from the live
    /// table are the fixture: every one of them must be read as a nickname.
    #[test]
    fn generated_grammar_accepts_only_auto_name_output() {
        for generated in [
            "ALL",
            "3ix:Buy",
            "3ix:Buy · max=1 · bkt=1",
            "2ix:B · fs_buy=19.5",
            "cu_limit=200K",
            "5ix:BuyExactSolIn · cu_limit=301K · cu_price=75210",
            "5ix:BuyExactSolIn · cu_limit=301K · cu_price=75.2K",
            "max=0.27 · bkt=0.00001",
            "1ix:Buy · max=1 · bkt=exact",
            "init=0 · bkt=1000",
            "fs_buy=2.5 · bkt=5",
        ] {
            assert!(is_generated_auto_name(generated), "`{generated}` is auto_name output");
        }
        for nickname in [
            "",
            "max-buy launcher",
            "8dtx · Trojan Trade",
            "8dtx · GMGN Bot",
            "8dtx-clone: creation bundle < 5 SOL",
            "8dtx-clone CONTROL: any creation bundle",
            "8dtx S1: Pump.Fun: BuyV2 + bundle<5",
            "8dtx-derived - any token (structural classifier)",
            "isl-ALL broad",
            "probe group mc0.0108 (held +17.13pc 9of9)",
            "buyv2 mc7.07 (x1.0226 tool, 1 SOL-tier sibling of g0)",
            // Chip-shaped but not a chip: an unknown axis, a bad number, no count.
            "cu_lmit=200K",
            "max=1.2.3",
            "ix:Buy",
            "bkt=wide",
        ] {
            assert!(!is_generated_auto_name(nickname), "`{nickname}` must read as a nickname");
        }
    }

    /// `auto_name` output is *stored*, so changing that function strands the copies
    /// already written and two identical fingerprints read as two. A grammar-shaped
    /// name that drifted must re-derive; a nickname must not.
    #[test]
    fn a_drifted_generated_name_re_derives_and_a_nickname_does_not() {
        let mut fp = bare();
        fp.cu_limit = Some(301_000);
        fp.cu_price = Some(75_210);
        fp.ix_labels = Some(vec!["A".into(), "Pump.Fun: BuyExactSolIn".into()]);

        // Written by an older `auto_name` that did not compact `cu_price`.
        fp.name = "2ix:BuyExactSolIn · cu_limit=301K · cu_price=75210".into();
        assert!(fp.has_stale_auto_name());
        fp.ensure_auto_name();
        assert_eq!(fp.name, "2ix:BuyExactSolIn · cu_limit=301K · cu_price=75.2K");

        // Already current — no rewrite, and no churn on repeated reads.
        assert!(!fp.has_stale_auto_name());

        // The nickname states a finding the axes cannot; it survives.
        fp.name = "probe group mc0.0108 (held +17.13pc 9of9)".into();
        assert!(!fp.has_stale_auto_name());
        fp.ensure_auto_name();
        assert_eq!(fp.name, "probe group mc0.0108 (held +17.13pc 9of9)");
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
