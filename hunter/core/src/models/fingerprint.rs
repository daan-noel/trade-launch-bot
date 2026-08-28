//! `Fingerprint` — a token-creation shape shared by many strategy rules. Backs the
//! `fingerprints` table.
//!
//! A fingerprint is a `wildcard` flag or a [`Criteria`] map: one
//! [`AxisPredicate`] per configured axis, keyed by [`AxisId`]. The axis registry
//! ([`hunter_engine::fingerprint::axis`]) says what each axis measures, how to read
//! it off a token, and when it settles — so this module never enumerates axes and a
//! new one lands here for free.
//!
//! **Match semantics live in the engine** ([`hunter_engine::fingerprint::matches_phase`]),
//! not here. This type is the storage/serialisation half: parse a wire body,
//! validate it, name it.
//!
//! Identity is `criteria` + `wildcard`. `name` is a label — a picker handle and a
//! log line, never part of what a fingerprint matches.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use hunter_engine::fingerprint::{
    AxisId, AxisKind, AxisPredicate, AxisUnit, Criteria, Fingerprint as EngineFingerprint,
    FingerprintId,
};
use hunter_engine::grouping::sol_label;

/// Read an optional integer field from an HTTP JSON body (accepts a JSON number or
/// a numeric string). Shared SSOT for the generic-engine CRUD parse paths.
pub fn opt_i64(body: &serde_json::Value, key: &str) -> Option<i64> {
    body.get(key).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

/// A `fingerprints` row. See module docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub id: Uuid,
    /// Human-facing label. Not identity.
    pub name: String,
    /// Match EVERY token, ignoring every axis.
    ///
    /// A rule always needs a fingerprint, but not every rule is about a creation
    /// shape — one deciding purely on what the tape is doing has none to name. An
    /// empty [`Self::criteria`] means *match nothing*, so "every token" has to be
    /// said out loud or it is indistinguishable from a half-filled form.
    #[serde(default)]
    pub wildcard: bool,
    /// The configured axes. Stored as one `JSONB` column, so a new axis needs no
    /// migration.
    #[serde(default)]
    pub criteria: Criteria,
    /// Per-metric-group fingerprint-side config (e.g. `m_flow_ix.ix_patterns`).
    /// **Not** part of match identity — it selects no token — but it IS part of ROW
    /// identity: it compiles into this fingerprint's live `m_flow_ix` patterns, so
    /// two rows matching the same tokens with different config are different
    /// fingerprints. `find_or_create` and the `fingerprints_identity_uniq` index both
    /// key on it.
    #[serde(default = "default_metric_config")]
    pub metric_config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_metric_config() -> serde_json::Value {
    serde_json::json!({})
}

impl Fingerprint {
    /// A criterion-less row — matches nothing until an axis or the wildcard is set.
    pub fn empty(id: Uuid, now: DateTime<Utc>) -> Self {
        Fingerprint {
            id,
            name: String::new(),
            wildcard: false,
            criteria: Criteria::new(),
            metric_config: default_metric_config(),
            created_at: now,
            updated_at: now,
        }
    }

    /// The pure matcher view of this row — **the one converter**, so the live entry
    /// gate and every mirror grade the same predicates. Drops the label and the
    /// timestamps; identity carries over verbatim because it is the same type.
    pub fn to_engine(&self) -> EngineFingerprint {
        EngineFingerprint {
            id: FingerprintId(self.id),
            wildcard: self.wildcard,
            criteria: self.criteria.clone(),
            metric_config: self.metric_config.clone(),
        }
    }

    /// Parse a fingerprint from a raw HTTP JSON body — the SSOT for the wire shape,
    /// shared by the live + lab CRUD handlers. `id` and the timestamps are
    /// caller-supplied, never read from the body.
    ///
    /// Two accepted spellings of `criteria`, both landing on the same map:
    ///
    /// * the canonical `{"max_cost_lamports": {"kind":"range","min":"1","max":"2"}}`
    /// * a bare `{"max_cost_lamports": {"min":"1","max":"2"}}` — the form a hand
    ///   -written script or a form POST produces, defaulted to a range because every
    ///   numeric axis has exactly one predicate shape today.
    ///
    /// An **unknown axis key or an unparseable predicate is an error**, never a
    /// silent drop: a dropped axis reads as "not part of identity", which *widens*
    /// what the fingerprint matches instead of failing the write.
    pub fn from_json(
        body: &serde_json::Value,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        let criteria = parse_criteria(body.get("criteria"))?;
        Ok(Fingerprint {
            id,
            name: body.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            wildcard: body.get("wildcard").and_then(|v| v.as_bool()).unwrap_or(false),
            criteria,
            metric_config: body
                .get("metric_config")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or_else(default_metric_config),
            created_at: now,
            updated_at: now,
        })
    }

    /// Whether any matchable criterion is configured.
    pub fn has_any_criterion(&self) -> bool {
        self.wildcard || !self.criteria.is_empty()
    }

    /// Whether any **axis** is configured, ignoring [`Self::wildcard`]. Separate
    /// from [`Self::has_any_criterion`] because the two ask opposite questions of a
    /// wildcard row: it always *has* a criterion, and it must never carry an axis.
    pub fn has_axis_criterion(&self) -> bool {
        !self.criteria.is_empty()
    }

    /// Whether any configured axis defers to the settled creation slot.
    pub fn has_first_slot_criteria(&self) -> bool {
        !self.wildcard && self.criteria.has_deferred()
    }

    /// **The ONE write-edge gate** for a persisted fingerprint — called by the live
    /// and lab create/update handlers (for a 400) and again by `FingerprintRepo`
    /// insert/update (the backstop for non-HTTP writers like sweep promotion). The
    /// `fingerprints_criteria_shape` CHECK is the last line of defence.
    ///
    /// Delegates to the engine's own gate so the two readers of one row can never
    /// disagree about whether it is usable.
    pub fn validate(&self) -> Result<(), String> {
        self.to_engine().validate()
    }

    /// Every reason this row is unusable, for a form that shows them all at once.
    pub fn problems(&self) -> Vec<String> {
        self.to_engine().problems()
    }

    /// Compact label from the configured axes — the one auto-name every create path
    /// uses (sweep promote, creation-stats, flow-discovery bind, blank form).
    /// Identity stays on the criteria; this is a picker/log handle.
    ///
    /// Generated **from the registry**, chip per axis in registry order, so a new
    /// axis is named without touching this function or its grammar
    /// ([`is_auto_name_chip`]).
    pub fn auto_name(&self) -> String {
        // A wildcard row has no axes to name (validation guarantees it), so it names
        // the token set it matches.
        if self.wildcard {
            return WILDCARD_NAME.into();
        }
        let parts: Vec<String> = self.criteria.iter().filter_map(|(id, p)| axis_chip(id, p)).collect();
        if parts.is_empty() {
            WILDCARD_NAME.into()
        } else {
            parts.join(AUTO_NAME_SEP)
        }
    }

    /// True when `name` is blank or a retired auto-label.
    pub fn has_legacy_auto_name(&self) -> bool {
        is_legacy_auto_name(&self.name)
    }

    /// True when `name` is an auto-label that no longer says what the axes say — a
    /// retired shape, or a **current-grammar** one that has since drifted from
    /// [`Self::auto_name`].
    ///
    /// The second case is what lets a naming change finish. `auto_name` is a pure
    /// function of the criteria, but its output is *stored*, so every edit to it
    /// strands the copies already written — two rows with identical criteria then
    /// read as two fingerprints, which is the whole problem the name exists to
    /// prevent. Deciding it by grammar ([`is_generated_auto_name`]) rather than by an
    /// ever-growing list of retired prefixes means the next change heals itself —
    /// including the retired `bkt=` width chip, which no longer parses.
    ///
    /// A nickname is not in the grammar, so it is never touched — it is the only
    /// record of *why* a fingerprint exists, and the axes can always be re-read.
    pub fn has_stale_auto_name(&self) -> bool {
        self.has_legacy_auto_name()
            || (is_generated_auto_name(&self.name) && self.name != self.auto_name())
    }

    /// Replace a blank / stale auto-label with [`Self::auto_name`]. A nickname is
    /// left untouched.
    pub fn ensure_auto_name(&mut self) {
        if self.has_stale_auto_name() {
            self.name = self.auto_name();
        }
    }
}

/// Parse the `criteria` object off a wire body.
fn parse_criteria(raw: Option<&serde_json::Value>) -> Result<Criteria, String> {
    let Some(raw) = raw else { return Ok(Criteria::new()) };
    if raw.is_null() {
        return Ok(Criteria::new());
    }
    let obj = raw.as_object().ok_or("criteria must be a JSON object of axis -> predicate")?;
    let mut out = Criteria::new();
    for (key, value) in obj {
        let axis = AxisId::from_key(key)
            .ok_or_else(|| format!("`{key}` is not a fingerprint axis"))?;
        // `null` is how a form clears an axis it previously set, so it means "not
        // configured" — the same state as omitting the key.
        if value.is_null() {
            continue;
        }
        let pred = parse_predicate(axis, value)?;
        // An all-open range configures nothing, and storing one would make a row read
        // as narrowed while matching everything the axis can hold. Treat it as
        // cleared, the same as `null`.
        if matches!(pred, AxisPredicate::Range { min: None, max: None }) {
            continue;
        }
        out.insert(axis, pred);
    }
    Ok(out)
}

/// One predicate, accepting the canonical tagged form and the bare shorthand.
fn parse_predicate(axis: AxisId, value: &serde_json::Value) -> Result<AxisPredicate, String> {
    let key = axis.key();
    let mut v = value.clone();
    if v.get("kind").is_none() {
        // Shorthand: infer the tag from the axis's own kind, so a caller writing
        // `{"min": 1}` cannot accidentally name a predicate the axis cannot carry.
        let kind = match axis.def().kind {
            AxisKind::Numeric => "range",
            AxisKind::Sequence => "sequence",
        };
        if let Some(obj) = v.as_object_mut() {
            obj.insert("kind".into(), serde_json::Value::String(kind.into()));
        } else if let (AxisKind::Sequence, Some(arr)) = (axis.def().kind, v.as_array()) {
            // A bare `["A","B"]` is the natural spelling of a label sequence.
            v = serde_json::json!({ "kind": "sequence", "labels": arr });
        } else {
            return Err(format!("`{key}`: expected a predicate object"));
        }
    }
    serde_json::from_value::<AxisPredicate>(v)
        .map_err(|e| format!("`{key}`: {e}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Auto-name grammar
// ─────────────────────────────────────────────────────────────────────────────

/// Chip separator. Mirrored by the TS `AUTO_NAME_SEP`.
pub const AUTO_NAME_SEP: &str = " · ";

/// Auto-name of a fingerprint with nothing to name from its axes: a `wildcard` row
/// (which matches every token) and — for the criterion-less draft the write edge
/// rejects — the same word, because both describe the same token set. Mirrored by
/// the TS `WILDCARD_NAME`.
pub const WILDCARD_NAME: &str = "ALL";

/// Separates the two bounds of a range chip. Deliberately not `-`: an amount chip
/// is a decimal, and `1-2` would read as a subtraction to a human and be ambiguous
/// against a negative bound to the grammar checker.
const RANGE_SEP: char = '~';

/// One axis's chip, or `None` when the axis names nothing renderable.
fn axis_chip(id: AxisId, pred: &AxisPredicate) -> Option<String> {
    match pred {
        // The label sequence keeps its own shape: the COUNT is what makes it
        // readable at chip size, with the trailing action for a hint of which tool.
        AxisPredicate::Sequence { labels } if id == AxisId::IxLabels => {
            Some(ix_labels_count_tail(labels))
        }
        AxisPredicate::Sequence { .. } => None,
        AxisPredicate::Range { min, max } => {
            let unit = id.def().unit;
            let n = |v: &u128| render_bound(*v, unit);
            let body = match (min, max) {
                (Some(a), Some(b)) if a == b => n(a),
                (Some(a), Some(b)) => format!("{}{RANGE_SEP}{}", n(a), n(b)),
                (Some(a), None) => format!("{}{RANGE_SEP}", n(a)),
                (None, Some(b)) => format!("{RANGE_SEP}{}", n(b)),
                (None, None) => return None,
            };
            Some(format!("{}={body}", id.def().chip))
        }
    }
}

/// One bound, in the axis's display unit. Lamports read as SOL (what the operator
/// typed); everything else reads as the integer it is, compacted so a 200000 CU
/// limit does not eat half the chip.
fn render_bound(v: u128, unit: AxisUnit) -> String {
    match unit {
        AxisUnit::Lamports => sol_label(v),
        AxisUnit::ComputeUnits => format_compact_int(v),
        AxisUnit::Count | AxisUnit::Labels => v.to_string(),
    }
}

/// Whether `name` is written in [`Fingerprint::auto_name`]'s own chip grammar:
/// every `AUTO_NAME_SEP`-separated part is a chip that function emits. Such a name
/// was generated, never typed, so [`Fingerprint::has_stale_auto_name`] may rewrite
/// it once it stops matching the axes.
///
/// Deliberately strict — an unrecognised part makes the whole name a nickname. The
/// cost of the two mistakes is not symmetric: re-deriving a name it declined to
/// touch is free, while rewriting a real nickname destroys the only record of why
/// that fingerprint was created. Mirrored by the TS `isGeneratedAutoName`.
pub fn is_generated_auto_name(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return false;
    }
    if n == WILDCARD_NAME {
        return true;
    }
    n.split(AUTO_NAME_SEP).all(is_auto_name_chip)
}

/// One chip of the [`is_generated_auto_name`] grammar. **Derived from the registry**,
/// so an axis added there is recognised here without an edit — the drift this used
/// to have (a chip emitted but not recognised, so its name never healed) is
/// structurally impossible now.
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
    let Some(axis) = AxisId::ALL.into_iter().find(|a| a.def().chip == label) else {
        return false;
    };
    let bound_ok = |s: &str| is_bound(s, axis.def().unit);
    match value.split_once(RANGE_SEP) {
        // `1.5~2`, `1.5~`, `~2` — at least one side present.
        Some((lo, hi)) => match (lo.is_empty(), hi.is_empty()) {
            (true, true) => false,
            (true, false) => bound_ok(hi),
            (false, true) => bound_ok(lo),
            (false, false) => bound_ok(lo) && bound_ok(hi),
        },
        None => bound_ok(value),
    }
}

/// One rendered bound: digits, at most one `.`, and — for a compute-unit axis — an
/// optional `K`/`M`/`G` scale suffix. Never signed: identity is a non-negative
/// integer, so a `-` in a chip means the name was typed.
fn is_bound(s: &str, unit: AxisUnit) -> bool {
    let body = match unit {
        AxisUnit::ComputeUnits => s.strip_suffix(['K', 'M', 'G']).unwrap_or(s),
        _ => s,
    };
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

/// Retired auto-name shapes. Mirrored in the TS `isLegacyAutoName`.
///
/// The last clause is what lets a **chip** retire. [`is_generated_auto_name`] is
/// deliberately strict — one unrecognised part makes the whole name a nickname — so
/// a name carrying a chip that no longer exists would otherwise be frozen as a
/// nickname and never heal. A name whose parts are all either current chips or
/// *known retired* ones was still generated, so it is safe to rewrite; genuinely
/// unknown text is still a nickname and still untouchable.
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
    if n.starts_with("c · ") || n.starts_with("f · ") || n.starts_with("s · ") {
        return true;
    }
    let parts: Vec<&str> = n.split(AUTO_NAME_SEP).collect();
    parts.iter().any(|p| is_retired_auto_name_chip(p))
        && parts.iter().all(|p| is_retired_auto_name_chip(p) || is_auto_name_chip(p))
}

/// A chip [`Fingerprint::auto_name`] used to emit and no longer does.
///
/// `bkt=…` was the row-wide SOL bucket width. It has no successor — a width is not
/// a property of a fingerprint any more, because each axis carries its own explicit
/// range — so a name holding one is stale by construction.
fn is_retired_auto_name_chip(part: &str) -> bool {
    match part.split_once('=') {
        Some(("bkt", v)) => v == "exact" || is_bound(v, AxisUnit::Count),
        _ => false,
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
fn format_compact_int(n: u128) -> String {
    if n >= 1_000_000_000 {
        format!("{}G", format_decimal_trim(n as f64 / 1_000_000_000.0, 1))
    } else if n >= 1_000_000 {
        format!("{}M", format_decimal_trim(n as f64 / 1_000_000.0, 1))
    } else if n >= 1_000 {
        format!("{}K", format_decimal_trim(n as f64 / 1_000.0, 1))
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOL: u128 = 1_000_000_000;

    fn fp(criteria: Criteria) -> Fingerprint {
        let now = Utc::now();
        Fingerprint { criteria, ..Fingerprint::empty(Uuid::nil(), now) }
    }

    fn one(axis: AxisId, pred: AxisPredicate) -> Fingerprint {
        fp(Criteria::new().with(axis, pred))
    }

    // ── Wire parsing ────────────────────────────────────────────────────────

    #[test]
    fn the_canonical_and_shorthand_predicate_forms_land_on_the_same_criteria() {
        let now = Utc::now();
        let canonical = serde_json::json!({
            "criteria": {
                "max_cost_lamports": { "kind": "range", "min": "1000000000", "max": "1999999999" },
                "ix_labels": { "kind": "sequence", "labels": ["A", "B"] },
            }
        });
        let shorthand = serde_json::json!({
            "criteria": {
                "max_cost_lamports": { "min": 1000000000, "max": 1999999999 },
                "ix_labels": ["A", "B"],
            }
        });
        let a = Fingerprint::from_json(&canonical, Uuid::nil(), now).unwrap();
        let b = Fingerprint::from_json(&shorthand, Uuid::nil(), now).unwrap();
        assert_eq!(a.criteria, b.criteria);
        assert_eq!(
            a.criteria.get(AxisId::MaxCostLamports),
            Some(&AxisPredicate::Range { min: Some(SOL), max: Some(2 * SOL - 1) })
        );
    }

    /// A key nobody recognises must FAIL the write. Dropping it would leave the axis
    /// unconfigured, i.e. widen what the row matches — the silent direction.
    #[test]
    fn an_unknown_axis_or_a_malformed_predicate_is_an_error_not_a_drop() {
        let now = Utc::now();
        let bad_key = serde_json::json!({ "criteria": { "cu_limitt": { "min": 1 } } });
        assert!(Fingerprint::from_json(&bad_key, Uuid::nil(), now).is_err());

        let bad_pred = serde_json::json!({ "criteria": { "cu_limit": "200000" } });
        assert!(Fingerprint::from_json(&bad_pred, Uuid::nil(), now).is_err());

        let bad_kind = serde_json::json!({
            "criteria": { "cu_limit": { "kind": "sequence", "labels": ["A"] } }
        });
        let parsed = Fingerprint::from_json(&bad_kind, Uuid::nil(), now).unwrap();
        assert!(parsed.validate().is_err(), "a sequence on a numeric axis must not validate");
    }

    /// `null` and an all-open range are the two ways a form says "cleared". Both
    /// collapse to absent, so one intent has one stored spelling.
    #[test]
    fn a_cleared_axis_has_exactly_one_stored_spelling() {
        let now = Utc::now();
        let body = serde_json::json!({
            "criteria": {
                "cu_limit": serde_json::Value::Null,
                "cu_price": { "kind": "range" },
                "ix_count": { "min": 3 },
            }
        });
        let parsed = Fingerprint::from_json(&body, Uuid::nil(), now).unwrap();
        assert!(!parsed.criteria.contains(AxisId::CuLimit));
        assert!(!parsed.criteria.contains(AxisId::CuPrice));
        assert_eq!(parsed.criteria.len(), 1);
    }

    #[test]
    fn a_body_with_no_criteria_is_a_criterion_less_draft_the_gate_rejects() {
        let now = Utc::now();
        let parsed = Fingerprint::from_json(&serde_json::json!({}), Uuid::nil(), now).unwrap();
        assert!(!parsed.has_any_criterion());
        assert!(parsed.validate().is_err());
    }

    // ── Validation ──────────────────────────────────────────────────────────

    #[test]
    fn validation_is_the_engines_own_gate() {
        assert!(one(AxisId::CuLimit, AxisPredicate::exact(200_000)).validate().is_ok());
        // Unsatisfiable.
        assert!(one(AxisId::CuLimit, AxisPredicate::range(Some(9), Some(1))).validate().is_err());
        // Wildcard plus an axis.
        let mut w = one(AxisId::CuLimit, AxisPredicate::exact(1));
        w.wildcard = true;
        assert!(w.validate().is_err());
        // Wildcard alone.
        let mut w = fp(Criteria::new());
        w.wildcard = true;
        assert!(w.validate().is_ok());
        assert!(w.has_any_criterion() && !w.has_axis_criterion());
    }

    #[test]
    fn deferral_comes_from_the_registry_not_a_local_list() {
        assert!(one(AxisId::FirstSlotBuyLamports, AxisPredicate::exact(1)).has_first_slot_criteria());
        assert!(!one(AxisId::CuLimit, AxisPredicate::exact(1)).has_first_slot_criteria());
        // A wildcard waits for nothing.
        let mut w = fp(Criteria::new());
        w.wildcard = true;
        assert!(!w.has_first_slot_criteria());
    }

    // ── Auto-name ───────────────────────────────────────────────────────────

    #[test]
    fn a_chip_is_emitted_per_axis_in_registry_order() {
        let f = fp(Criteria::new()
            .with(AxisId::IxLabels, AxisPredicate::Sequence {
                labels: vec!["Pump.Fun: Create".into(), "Pump.Fun: Buy".into()],
            })
            .with(AxisId::CuLimit, AxisPredicate::exact(200_000))
            .with(AxisId::MaxCostLamports, AxisPredicate::range(Some(SOL), Some(2 * SOL)))
            .with(AxisId::PriorLaunches, AxisPredicate::exact(0)));
        assert_eq!(f.auto_name(), "cu_limit=200K · max=1~2 · 2ix:Buy · prior=0");
    }

    #[test]
    fn open_ended_and_exact_chips_read_differently() {
        assert_eq!(one(AxisId::InitBuyLamports, AxisPredicate::exact(1_515_000_000)).auto_name(), "init=1.515");
        assert_eq!(one(AxisId::InitBuyLamports, AxisPredicate::range(Some(2 * SOL), None)).auto_name(), "init=2~");
        assert_eq!(one(AxisId::InitBuyLamports, AxisPredicate::range(None, Some(2 * SOL))).auto_name(), "init=~2");
        assert_eq!(one(AxisId::IxCount, AxisPredicate::range(Some(3), Some(5))).auto_name(), "ix_count=3~5");
        // A ceiling names itself exactly rather than being rounded into prose.
        assert_eq!(
            one(AxisId::MaxCostLamports, AxisPredicate::exact(u128::from(u64::MAX))).auto_name(),
            "max=18446744073.709551615"
        );
    }

    #[test]
    fn a_wildcard_and_a_blank_row_both_name_the_token_set_they_describe() {
        let mut w = fp(Criteria::new());
        w.wildcard = true;
        assert_eq!(w.auto_name(), WILDCARD_NAME);
        assert_eq!(fp(Criteria::new()).auto_name(), WILDCARD_NAME);
    }

    /// The property that lets a naming change finish: everything `auto_name` emits,
    /// the grammar recognises — so a stored name that drifts is rewritten, and a
    /// nickname never is. Walks the whole registry, so a new axis is covered.
    #[test]
    fn every_emitted_name_is_recognised_by_its_own_grammar() {
        for axis in AxisId::ALL {
            let preds = match axis.def().kind {
                AxisKind::Sequence => vec![AxisPredicate::Sequence {
                    labels: vec!["Pump.Fun: Create".into(), "System Program: Transfer".into()],
                }],
                AxisKind::Numeric => vec![
                    AxisPredicate::exact(0),
                    AxisPredicate::exact(1_515_000_000),
                    AxisPredicate::exact(u128::from(u64::MAX)),
                    AxisPredicate::range(Some(1), Some(200_000)),
                    AxisPredicate::range(Some(3), None),
                    AxisPredicate::range(None, Some(3)),
                ],
            };
            for pred in preds {
                let name = one(axis, pred.clone()).auto_name();
                assert!(
                    is_generated_auto_name(&name),
                    "{axis:?} emits {name:?}, which its own grammar rejects"
                );
                let mut f = one(axis, pred);
                f.name = name.clone();
                assert!(!f.has_stale_auto_name(), "{name:?} must be stable once written");
            }
        }
    }

    #[test]
    fn a_nickname_survives_and_a_drifted_generated_name_heals() {
        let mut f = one(AxisId::CuLimit, AxisPredicate::exact(200_000));
        f.name = "8dtx router".into();
        assert!(!f.has_stale_auto_name());
        f.ensure_auto_name();
        assert_eq!(f.name, "8dtx router", "a nickname is the only record of WHY");

        // A generated name that no longer matches the axes is rewritten.
        f.name = "cu_limit=999K".into();
        assert!(f.has_stale_auto_name());
        f.ensure_auto_name();
        assert_eq!(f.name, "cu_limit=200K");
    }

    /// The retired width chip is not in the grammar, so a name carrying one is
    /// stale by construction and heals on the next read — no backfill needed.
    #[test]
    fn a_retired_bucket_width_name_heals_itself() {
        let mut f = one(AxisId::InitBuyLamports, AxisPredicate::range(Some(SOL), Some(2 * SOL - 1)));
        f.name = "init=1 · bkt=0.5".into();
        assert!(!is_auto_name_chip("bkt=0.5"), "the width chip is retired");
        assert!(f.has_stale_auto_name());
        f.ensure_auto_name();
        assert_eq!(f.name, "init=1~1.999999999");
    }

    #[test]
    fn legacy_labels_are_still_recognised() {
        for n in ["", "  ", "flow-discovery bind", "sweep 12ab · group 3", "c · x", "f · y", "s · z"] {
            assert!(is_legacy_auto_name(n), "{n:?}");
        }
        assert!(!is_legacy_auto_name("8dtx router"));
    }

    #[test]
    fn the_engine_view_carries_identity_verbatim() {
        let f = one(AxisId::MaxCostLamports, AxisPredicate::exact(u128::from(u64::MAX)));
        let e = f.to_engine();
        assert_eq!(e.id.0, f.id);
        assert_eq!(e.criteria, f.criteria);
        assert_eq!(e.wildcard, f.wildcard);
    }
}
