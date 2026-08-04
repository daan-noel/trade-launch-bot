//! Strategy-agnostic token grouping.
//!
//! A sweep can partition its corpus by a **compound fingerprint key** — the
//! exact values of one or more token-creation fields the user picks on the page
//! (CU settings, the creation-instruction `max_cost_lamports` / `spendable_lamports_in`,
//! the instruction-label set, …). Each surviving group is swept independently so
//! the UI can answer "for tokens with *this* fingerprint, which param combo is
//! best?".
//!
//! Creator wallet is **deliberately not** a grouping dimension: on pump.fun
//! creators rotate wallets constantly, so a creator key is un-trackable across
//! tokens and only ever yields singleton groups. It was removed to keep any
//! creator-wallet classification out of the project entirely.
//!
//! This module is intentionally strategy-blind: it only reads
//! [`TokenFingerprint`] (carried on each corpus token) and never touches the
//! `Strategy`/`ParamSpace` surface — so Swing Detection groups by the same keys.
//!
//! Grouping is **exact-value** for the discrete fields (program id, CU limit/price,
//! cashback, ix-labels) and **binned** for the continuous SOL-amount fields (initial
//! buy, max cost, spendable-in, first-slot buy/sell) — these are effectively
//! continuous, so exact grouping would make every token its own group. Binning lives
//! entirely inside [`render_field`] (via [`bucket_sol_label`]) at a **per-run** width
//! passed into [`group_key`] (default [`SOL_BUCKET_WIDTH`]) — no engine or schema change.
//! See `creation_stats_repo` for the matching dashboard SQL bin (kept in lockstep at the
//! same width so both surfaces produce identical labels).

use serde::{Deserialize, Serialize};
use serde_json::Value;


/// Sentinel rendered for a missing (`None`/empty) field value, so tokens that
/// lack the field form their own group instead of colliding with `0`/`""`.
const MISSING: &str = "∅";

/// Bucket width (SOL) for the continuous SOL-amount grouping fields (initial buy,
/// max cost, spendable-in, first-slot buy/sell). These are effectively continuous
/// values, so exact-value grouping yields one token per group; binning them into
/// fixed-width ranges makes them useful group keys. Must stay in lockstep with the
/// dashboard SQL bin in `creation_stats_repo` (both render the same `"lo–hi"` label).
pub const SOL_BUCKET_WIDTH: f64 = 0.1;

/// Decimal places to render each bucket edge at — derived from [`SOL_BUCKET_WIDTH`]
/// (0.1 → 1 place). Mirrored by the dashboard SQL `to_char` format.
pub const SOL_BUCKET_DECIMALS: usize = 1;

/// The `[lo, hi)` bucket ratio-epsilon (see [`bucket_index`]). Shared by the
/// dashboard SQL bin so both sides place on-edge values identically.
pub const BUCKET_EPS: f64 = 1e-9;

/// Lamports per SOL as `f64` — the one in-engine lamports→SOL divisor. The engine
/// is pure (no `config::constants`), so this is the SSOT both the grouping labels
/// and the [`crate::fingerprint`] bucket matcher divide by.
pub const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;

/// Smallest legal bucket width (SOL). Below this the `1e-9` ratio-epsilon stops
/// being negligible and labels blow up in decimals, so rule validation rejects
/// anything smaller. `1e-6 SOL = 1000 lamports`, far finer than any real config.
pub const MIN_BUCKET_WIDTH_SOL: f64 = 1e-6;

/// **The one bucketing primitive.** Maps a value to its half-open bucket index
/// `floor(v / width)`, where bucket `i` covers `[i·width, (i+1)·width)`. Every
/// bucket label, membership test, and dashboard bin derives from this so they
/// can never disagree.
///
/// **Boundary robustness:** `0.1` is not exactly representable in f64, so a naive
/// `floor(v / 0.1)` misplaces edge values (e.g. `0.3 / 0.1 == 2.9999999996 →`
/// wrong bucket). The `+ BUCKET_EPS` nudge on the ratio absorbs that float noise
/// so an on-edge value lands in the upper bucket. `1e-9` (in ratio units = 0.1
/// lamport at width 0.1) is far below the 1-lamport quantum of any real SOL
/// amount, so it never promotes a genuinely sub-edge value. The dashboard SQL
/// applies the identical epsilon so sweep and dashboard bin identically.
pub fn bucket_index(v: f64, width: f64) -> i64 {
    ((v / width) + BUCKET_EPS).floor() as i64
}

/// Whether two values fall in the same [`bucket_index`] bucket at `width`. This
/// is the **matcher** membership test — the live `on_token_created` hot path — so
/// it is alloc-free (no label string), unlike [`bucket_sol_label`].
pub fn same_bucket(a: f64, b: f64, width: f64) -> bool {
    bucket_index(a, width) == bucket_index(b, width)
}

/// Decimal places needed to render a bucket edge at `width` without loss:
/// `1.0 → 0`, `0.5 → 1`, `0.1 → 1`, `0.25 → 2`, `0.05 → 2`, `5.0 → 0`. The
/// dashboard SQL derives the same precision so labels stay byte-identical.
pub fn decimals_for(width: f64) -> usize {
    let mut d = 0usize;
    let mut w = width.abs();
    while d < 12 && w.fract().abs() > BUCKET_EPS {
        w *= 10.0;
        d += 1;
    }
    d
}

/// Render a SOL value as its half-open bucket label `"lo–hi"` — e.g. with
/// `width = 0.1`, `2.34 → "2.3–2.4"`. The bucket is `[lo, hi)` where
/// `lo = bucket_index(v, width) · width`. Built on [`bucket_index`] so the label
/// and the [`same_bucket`] membership test can never drift. The dashboard SQL
/// applies the identical epsilon + `to_char` rounding for byte-identical labels.
pub fn bucket_sol_label(v: f64, width: f64, decimals: usize) -> String {
    let lo = bucket_index(v, width) as f64 * width;
    let hi = lo + width;
    format!("{lo:.decimals$}–{hi:.decimals$}")
}

/// Bucket a lamports amount by converting to human SOL first, so the label reads in
/// SOL (matching the field's "SOL cost"/"SOL in" display name) rather than raw
/// lamports. Same binning as [`bucket_sol_label`]. `width` is the per-run bucket
/// width; `decimals` must be [`decimals_for`]`(width)` so the label edge (always a
/// multiple of `width`) renders exactly — see [`render_field`].
fn bucket_lamports_as_sol(lamports: i64, width: f64, decimals: usize) -> String {
    bucket_sol_label(lamports as f64 / LAMPORTS_PER_SOL_F64, width, decimals)
}

/// How the continuous SOL axes are keyed for one run.
///
/// `Bucket(width)` is the normal mode — values collapse into `width`-wide `"lo–hi"`
/// ranges so they make useful group keys. `Exact` renders the amount itself, for
/// the "what are the most common exact dev-buy sizes?" question that bucketing by
/// construction cannot answer.
///
/// **This is a mode, not a sentinel width.** `0` is not spelled here and never
/// should be: a width is a *measured* quantity (see the zero-as-unbound rule in
/// the product `CLAUDE.md`), `0` is a division by zero in [`bucket_index`], and a
/// magic-`0` width already caused one live bug where two readers disagreed about
/// whether it meant "default 0.1" or "literally zero".
///
/// `From<f64>` exists so every existing `…(width)` call site keeps compiling and
/// keeps its meaning — only a caller that explicitly wants `Exact` says so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SolPrecision {
    /// Bin into `width`-wide half-open ranges.
    Bucket(f64),
    /// Key on the exact amount — one group per distinct value.
    Exact,
}

impl From<f64> for SolPrecision {
    fn from(width: f64) -> Self {
        SolPrecision::Bucket(width)
    }
}

impl SolPrecision {
    /// **The one reader of a stored bucket width.** `None` — and defensively any
    /// non-finite or non-positive width — resolves to [`SolPrecision::Exact`].
    ///
    /// Both `Fingerprint` structs (the `hunter-core` DB model and the engine's
    /// match-time copy) route through this, so the live entry gate and the
    /// dashboard's SQL mirror can never disagree about what a stored width means.
    /// The defensive arm matters because this value reaches the entry gate: a `0`
    /// slipping past `validate` + the `0020` CHECK would otherwise divide by zero
    /// in [`bucket_index`] and saturate every amount into one bucket — arming on
    /// every token. Failing to *exact* instead fails closed.
    pub fn from_width(width: Option<f64>) -> Self {
        match width {
            Some(w) if w.is_finite() && w > 0.0 => SolPrecision::Bucket(w),
            _ => SolPrecision::Exact,
        }
    }

    /// The bucket width, or `None` in [`SolPrecision::Exact`] mode. Use this at a
    /// boundary that must persist or echo the width (the sweep run row, the
    /// dashboard response) — `None` is the honest "not bucketed" there.
    pub fn width(self) -> Option<f64> {
        match self {
            SolPrecision::Bucket(w) => Some(w),
            SolPrecision::Exact => None,
        }
    }
}

/// Render a lamports amount as its **exact** human-SOL group key — the
/// [`SolPrecision::Exact`] counterpart of [`bucket_sol_label`].
///
/// Nine decimals is lossless for lamports; trailing zeros are trimmed so the key
/// reads as the amount a human typed (`1515000000 → "1.515"`, not `"1.515000000"`)
/// and so it round-trips through the filter box. The dashboard SQL mirrors this
/// byte-for-byte in `creation_stats_repo::sol_exact_sql` — keep the two in step,
/// exactly as `sol_bucket_sql` mirrors `bucket_sol_label`.
pub fn exact_sol_label(lamports: i64) -> String {
    let s = format!("{:.9}", lamports as f64 / LAMPORTS_PER_SOL_F64);
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() { "0".to_string() } else { t.to_string() }
}

/// Human SOL → lamports, **rounded**. The engine's own copy of the conversion
/// `trading_core::config::constants::sol_to_lamports` performs, because this crate
/// is pure (no `config`) by design — locked equal by trading_core's
/// `engine_sol_to_lamports_matches_the_repo_boundary_conversion`. Rounding (not
/// truncation) is what makes a lamports→SOL→lamports round-trip exact, so a filter
/// typed from a displayed amount recovers the stored integer.
pub fn sol_to_lamports(sol: f64) -> i64 {
    (sol * LAMPORTS_PER_SOL_F64).round() as i64
}

/// A parsed value-filter entry for one of the bucketed SOL axes
/// ([`GroupField::is_bucketed`]). **The one parser** for that syntax — the
/// dashboard SQL (`creation_stats_repo::field_filter_pred`), the resident-corpus
/// retain (`grouped_sweep::matches_field_filter`), and the request validator all
/// read it, so a value can never mean different things to different surfaces.
///
/// Two accepted forms, both in **human SOL**:
///
/// * `"1.515"` → [`SolFilter::Exact`] — that amount and nothing else, whatever the
///   run's bucket width. This is what you want to pin one dev-buy size.
/// * `"1.5–1.6"` (en-dash, or a plain `-`) → [`SolFilter::Range`] — the half-open
///   `[lo, hi)` window, i.e. exactly what a group chip displays. Copy a chip's text
///   into the filter box and it selects that chip's tokens.
///
/// Ranges are half-open on purpose: it makes `bucket_sol_label(v, w)` pasted back
/// as a filter select precisely `same_bucket(v, ·, w)`, with no double-counting at
/// the shared edge between adjacent buckets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolFilter {
    /// One exact amount, in lamports.
    Exact(i64),
    /// Half-open `[lo, hi)` in lamports.
    Range(i64, i64),
}

impl SolFilter {
    /// Parse one filter entry. `None` if it is neither an amount nor a range
    /// (callers surface that as a 400 rather than silently dropping it — a dropped
    /// value reads as "no filter", which is how this whole family failed before).
    pub fn parse(text: &str) -> Option<SolFilter> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        // Range first: split on the label's en-dash, or an ASCII hyphen for the
        // keyboard-typed form. Amounts here are non-negative, so a separator can
        // never be confused with a sign. `splitn(2)` keeps `1.5-1.6-1.7` invalid
        // rather than silently reading a prefix.
        for sep in ['–', '-'] {
            if let Some((lo, hi)) = t.split_once(sep) {
                let (lo, hi) = (lo.trim(), hi.trim());
                if lo.is_empty() || hi.is_empty() {
                    continue;
                }
                let (lo, hi) = (parse_sol(lo)?, parse_sol(hi)?);
                if hi <= lo {
                    return None;
                }
                return Some(SolFilter::Range(sol_to_lamports(lo), sol_to_lamports(hi)));
            }
        }
        Some(SolFilter::Exact(sol_to_lamports(parse_sol(t)?)))
    }

    /// Whether a lamports amount satisfies this filter.
    pub fn matches(self, lamports: i64) -> bool {
        match self {
            SolFilter::Exact(v) => lamports == v,
            SolFilter::Range(lo, hi) => lamports >= lo && lamports < hi,
        }
    }
}

/// A finite, non-negative human-SOL literal. Rejects `NaN`/`inf`/negatives so they
/// can never reach [`sol_to_lamports`] (where they'd saturate to a junk `i64`).
fn parse_sol(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// Token-creation metadata used **only** for grouping — never read by any
/// `simulate()`. Carried on each corpus token so grouping is a pure in-memory
/// pass with no extra DB hit in the sweep loop.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenFingerprint {
    pub token_program_id: Option<String>,
    pub initial_buy_sol: Option<f64>,
    pub cu_limit: Option<i64>,
    pub cu_price: Option<i64>,
    pub is_cashback_enabled: bool,
    /// Creation-instruction `max_cost_lamports`, in lamports.
    pub max_cost_lamports: Option<i64>,
    /// Creation-instruction `spendable_lamports_in`, in lamports.
    pub spendable_lamports_in: Option<i64>,
    /// Total buy SOL across trades landing in the token's creation slot (human
    /// SOL). First **trade-derived** fingerprint field — sourced from `tokens_info`,
    /// not the `tokens` creation row like every field above.
    pub first_slot_buy_sol: Option<f64>,
    /// Total sell SOL across trades landing in the token's creation slot (human SOL).
    pub first_slot_sell_sol: Option<f64>,
    /// Creation instruction-label sequence (exact on-chain order + count).
    pub ix_labels: Vec<String>,
}

/// Read a lamports value (u64 number or numeric string) from a creation
/// instruction-args object. Mirrors the live entry path's `instruction_arg_as_sol`
/// reader (minus the SOL conversion — grouping keys on raw lamports).
pub fn extract_lamports(instruction: Option<&Value>, key: &str) -> Option<i64> {
    let obj = instruction?;
    obj.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
}

/// Collect an instruction-labels JSON value into a `Vec<String>`, preserving
/// exact on-chain order and count. Accepts both persisted shapes:
/// bare `["A","B"]` and `{ "instructions": ["A","B"] }` (same dual-shape
/// contract as `tokens`/`trades` SQL helpers and the Tokens UI).
pub fn normalize_labels(labels: &Value) -> Vec<String> {
    let arr = match labels {
        Value::Array(a) => a.as_slice(),
        Value::Object(o) => match o.get("instructions") {
            Some(Value::Array(a)) => a.as_slice(),
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    arr.iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

/// Collect a label `Vec` into its canonical fingerprint form. Preserves exact
/// on-chain order and count — no dedup — so tokens that differ only in the
/// number of repeated instructions (e.g. one vs two "System Program: Transfer")
/// land in separate fingerprint groups, consistent with the rule's exact-ordered
/// ix_labels match. Shared by [`normalize_labels`] and the grouped-sweep
/// ix-labels corpus filter.
pub fn normalize_label_vec(v: Vec<String>) -> Vec<String> {
    v
}

/// One selectable grouping field. Serde snake_case tags match the `group_by`
/// array the frontend sends and the keys stored in a [`GroupKey`]'s JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupField {
    TokenProgramId,
    CuLimit,
    CuPrice,
    IsCashbackEnabled,
    MaxCostLamports,
    SpendableLamportsIn,
    InitialBuySol,
    FirstSlotBuySol,
    FirstSlotSellSol,
    IxLabels,
}

impl GroupField {
    /// Stable key used in the `GroupKey` JSON object (matches the serde tag).
    pub fn as_str(self) -> &'static str {
        match self {
            GroupField::TokenProgramId => "token_program_id",
            GroupField::CuLimit => "cu_limit",
            GroupField::CuPrice => "cu_price",
            GroupField::IsCashbackEnabled => "is_cashback_enabled",
            GroupField::MaxCostLamports => "max_cost_lamports",
            GroupField::SpendableLamportsIn => "spendable_lamports_in",
            GroupField::InitialBuySol => "initial_buy_sol",
            GroupField::FirstSlotBuySol => "first_slot_buy_sol",
            GroupField::FirstSlotSellSol => "first_slot_sell_sol",
            GroupField::IxLabels => "ix_labels",
        }
    }

    /// Parse a serde snake_case tag back to a field (inverse of [`as_str`]).
    /// Used by query handlers that take a comma-separated `group_by` string.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag.trim() {
            "token_program_id" => GroupField::TokenProgramId,
            "cu_limit" => GroupField::CuLimit,
            "cu_price" => GroupField::CuPrice,
            "is_cashback_enabled" => GroupField::IsCashbackEnabled,
            "max_cost_lamports" => GroupField::MaxCostLamports,
            "spendable_lamports_in" => GroupField::SpendableLamportsIn,
            "initial_buy_sol" => GroupField::InitialBuySol,
            "first_slot_buy_sol" => GroupField::FirstSlotBuySol,
            "first_slot_sell_sol" => GroupField::FirstSlotSellSol,
            "ix_labels" => GroupField::IxLabels,
            _ => return None,
        })
    }

    /// Whether this field's group key is a **bucket label** (`"1.5–1.6"`) rather
    /// than its exact value — i.e. the continuous SOL amounts [`render_field`]
    /// routes through `bucket_sol_label`/`bucket_lamports_as_sol`.
    ///
    /// **Why this exists.** A value *filter* on one of these fields cannot compare
    /// against the rendered group key (no typed amount ever equals `"1.5–1.6"`), so
    /// every filter surface has to special-case them and pin the underlying amount
    /// instead. Two surfaces do that — the dashboard's SQL
    /// (`creation_stats_repo::field_filter_pred`) and the grouped sweep's in-memory
    /// retain (`grouped_sweep::matches_field_filter`) — and they used to disagree
    /// about which fields were bucketed *and* in what unit, which made the filter
    /// silently unsatisfiable on both. This is the ONE decider; `render_field` and
    /// this function are locked together by `is_bucketed_agrees_with_render_field`.
    ///
    /// The frontend mirrors it as `BUCKETED_GROUP_FIELDS` (`groupedTypes.ts`).
    pub fn is_bucketed(self) -> bool {
        matches!(
            self,
            GroupField::MaxCostLamports
                | GroupField::SpendableLamportsIn
                | GroupField::InitialBuySol
                | GroupField::FirstSlotBuySol
                | GroupField::FirstSlotSellSol
        )
    }

    /// Every field, for exhaustive iteration in guard tests + filter validation.
    pub const ALL: [GroupField; 10] = [
        GroupField::TokenProgramId,
        GroupField::CuLimit,
        GroupField::CuPrice,
        GroupField::IsCashbackEnabled,
        GroupField::MaxCostLamports,
        GroupField::SpendableLamportsIn,
        GroupField::InitialBuySol,
        GroupField::FirstSlotBuySol,
        GroupField::FirstSlotSellSol,
        GroupField::IxLabels,
    ];
}

/// A compound group key: the chosen fields' exact values in selection order.
/// Stringified so any field type collapses to one hashable/comparable key and
/// round-trips to JSON for storage + the UI. An empty `Vec` is the single "ALL"
/// group (no grouping selected ⇒ one global sweep, same as the legacy table).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupKey(pub Vec<(GroupField, String)>);

impl GroupKey {
    /// `{"token_program_id":"Tokenkeg…","max_cost_lamports":"12345"}` — stored on the
    /// group row and rendered as chips by the page. Empty key ⇒ `{}`.
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.0.len());
        for (f, v) in &self.0 {
            map.insert(f.as_str().to_string(), Value::String(v.clone()));
        }
        Value::Object(map)
    }
}

/// Compute the group key for one token's fingerprint under the chosen fields at
/// the given [`SolPrecision`] (the per-run partition precision for the continuous
/// SOL fields; discrete fields ignore it). An `f64` converts in, so
/// `group_key(fp, fields, SOL_BUCKET_WIDTH)` still reads as before; pass
/// [`SolPrecision::Exact`] for one group per distinct amount.
pub fn group_key<P: Into<SolPrecision>>(
    fp: &TokenFingerprint,
    fields: &[GroupField],
    precision: P,
) -> GroupKey {
    let precision = precision.into();
    // Derive decimals once from the width so every continuous field renders its
    // (width-multiple) edge exactly — the invariant that keeps the label byte-for-byte
    // identical to the dashboard SQL `to_char` (see `creation_stats_repo::sol_bucket_sql`).
    let decimals = precision.width().map(decimals_for).unwrap_or(0);
    GroupKey(fields.iter().map(|f| (*f, render_field(fp, *f, precision, decimals))).collect())
}

/// Group-key string for one field. `None`/empty → [`MISSING`].
///
/// Discrete fields (program id, CU limit/price, cashback, ix-labels) render their
/// **exact** value. The continuous SOL-amount fields (initial buy, max cost,
/// spendable-in, first-slot buy/sell) are **binned** into `width`-wide ranges at
/// `decimals` = [`decimals_for`]`(width)` places — this is the single seam where a
/// value becomes a coarse, groupable key — unless the run asked for
/// [`SolPrecision::Exact`], where they render the amount itself.
fn render_field(
    fp: &TokenFingerprint,
    f: GroupField,
    precision: SolPrecision,
    decimals: usize,
) -> String {
    let opt = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| MISSING.to_string());
    let miss = || MISSING.to_string();
    // The precision policy, written once per source unit rather than per field —
    // the `Bucket` arms are byte-identical to what they were before `Exact`
    // existed, so adding the mode cannot perturb an existing group key.
    let key_lamports = |l: i64| match precision {
        SolPrecision::Bucket(w) => bucket_lamports_as_sol(l, w, decimals),
        SolPrecision::Exact => exact_sol_label(l),
    };
    let key_sol = |v: f64| match precision {
        SolPrecision::Bucket(w) => bucket_sol_label(v, w, decimals),
        SolPrecision::Exact => exact_sol_label(sol_to_lamports(v)),
    };
    match f {
        GroupField::TokenProgramId => {
            fp.token_program_id.clone().unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::CuLimit => opt(fp.cu_limit),
        GroupField::CuPrice => opt(fp.cu_price),
        GroupField::IsCashbackEnabled => fp.is_cashback_enabled.to_string(),
        // Continuous SOL amounts → binned SOL ranges. Lamports fields convert to SOL
        // first so the label reads in SOL (matching their "SOL cost"/"SOL in" name).
        GroupField::MaxCostLamports => fp.max_cost_lamports.map(key_lamports).unwrap_or_else(miss),
        GroupField::SpendableLamportsIn => {
            fp.spendable_lamports_in.map(key_lamports).unwrap_or_else(miss)
        }
        GroupField::InitialBuySol => fp.initial_buy_sol.map(key_sol).unwrap_or_else(miss),
        GroupField::FirstSlotBuySol => fp.first_slot_buy_sol.map(key_sol).unwrap_or_else(miss),
        GroupField::FirstSlotSellSol => fp.first_slot_sell_sol.map(key_sol).unwrap_or_else(miss),
        // Empty ≡ absent, decided by the same SSOT the fingerprint matcher uses —
        // this group key is read back into a fingerprint identity (`∅` ⇒ axis
        // unset), so the token side and the fingerprint side must agree.
        GroupField::IxLabels => match crate::fingerprint::configured_labels(Some(&fp.ix_labels)) {
            None => MISSING.to_string(),
            Some(labels) => labels.join(" | "),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fp() -> TokenFingerprint {
        TokenFingerprint {
            token_program_id: Some("Tokenkeg".into()),
            initial_buy_sol: Some(1.5),
            cu_limit: Some(200_000),
            cu_price: None,
            is_cashback_enabled: true,
            max_cost_lamports: Some(1_000_000_000),
            spendable_lamports_in: None,
            first_slot_buy_sol: Some(2.25),
            first_slot_sell_sol: None,
            ix_labels: vec!["Pump.Fun: Create".into(), "System: Transfer".into()],
        }
    }

    /// A group chip's own text, pasted back into the filter box, must select
    /// exactly that chip's tokens — the property that makes range syntax
    /// discoverable (the UI already shows you the syntax).
    #[test]
    fn a_bucket_label_round_trips_as_a_range_filter() {
        let width = SOL_BUCKET_WIDTH;
        let decimals = decimals_for(width);
        for sol in [0.0, 0.05, 1.515, 2.34, 15.15, 99.999] {
            let label = bucket_sol_label(sol, width, decimals);
            let filter = SolFilter::parse(&label).expect("a chip label must parse");
            let lamports = sol_to_lamports(sol);
            assert!(filter.matches(lamports), "{label} must contain its own value {sol}");
            // …and agree with the membership test the live matcher uses.
            for other in [0.0, 0.05, 1.515, 2.34, 15.15, 99.999] {
                assert_eq!(
                    filter.matches(sol_to_lamports(other)),
                    same_bucket(sol, other, width),
                    "range {label} disagrees with same_bucket on {other}"
                );
            }
        }
    }

    /// Exact mode's key must read as the amount a human would type, and be the
    /// value its own exact filter selects — so a card and a filter agree.
    #[test]
    fn exact_mode_keys_read_as_typed_amounts() {
        assert_eq!(exact_sol_label(1_515_000_000), "1.515");
        assert_eq!(exact_sol_label(15_150_000_000), "15.15");
        assert_eq!(exact_sol_label(1_000_000_000), "1");
        // Whole-SOL amounts must not have their integer digits eaten by the
        // trailing-zero trim — the `.` guard is what stops that.
        assert_eq!(exact_sol_label(10_000_000_000), "10");
        assert_eq!(exact_sol_label(100_000_000_000), "100");
        assert_eq!(exact_sol_label(0), "0");
        assert_eq!(exact_sol_label(1), "0.000000001"); // one lamport, lossless
        // The key round-trips through the shared filter parser.
        for l in [0_i64, 1, 50_000_000, 1_515_000_000, 15_150_000_000] {
            let key = exact_sol_label(l);
            assert_eq!(
                SolFilter::parse(&key),
                Some(SolFilter::Exact(l)),
                "exact key {key} must parse back to its own amount"
            );
        }
    }

    /// Exact mode splits what bucket mode merges — the whole point of the mode.
    #[test]
    fn exact_mode_separates_values_inside_one_bucket() {
        let mut a = fp();
        a.max_cost_lamports = Some(1_515_000_000); // 1.515
        let mut b = fp();
        b.max_cost_lamports = Some(1_520_000_000); // 1.520 — same [1.5,1.6) bucket
        let fields = [GroupField::MaxCostLamports];
        assert_eq!(
            group_key(&a, &fields, SOL_BUCKET_WIDTH),
            group_key(&b, &fields, SOL_BUCKET_WIDTH),
            "bucket mode must merge them"
        );
        assert_ne!(
            group_key(&a, &fields, SolPrecision::Exact),
            group_key(&b, &fields, SolPrecision::Exact),
            "exact mode must separate them"
        );
        assert_eq!(SolPrecision::Exact.width(), None);
        assert_eq!(SolPrecision::from(0.1).width(), Some(0.1));
    }

    #[test]
    fn sol_filter_parses_both_forms_and_rejects_junk() {
        assert_eq!(SolFilter::parse("1.515"), Some(SolFilter::Exact(1_515_000_000)));
        assert_eq!(SolFilter::parse(" 1.515 "), Some(SolFilter::Exact(1_515_000_000)));
        // En-dash (what a chip renders) and the typed ASCII hyphen are the same.
        let r = Some(SolFilter::Range(1_500_000_000, 1_600_000_000));
        assert_eq!(SolFilter::parse("1.5–1.6"), r);
        assert_eq!(SolFilter::parse("1.5-1.6"), r);
        assert_eq!(SolFilter::parse("1.5 – 1.6"), r);
        // Half-open: lo is in, hi belongs to the next bucket.
        let f = SolFilter::parse("1.5–1.6").unwrap();
        assert!(f.matches(1_500_000_000) && f.matches(1_599_999_999));
        assert!(!f.matches(1_600_000_000) && !f.matches(1_499_999_999));
        // Junk is rejected, never silently dropped into "no filter".
        for bad in ["", "  ", "abc", "1.5–", "–1.6", "1.6–1.5", "1.5–1.5", "1.5-1.6-1.7", "-1", "NaN", "inf"] {
            assert_eq!(SolFilter::parse(bad), None, "{bad:?} must not parse");
        }
    }

    /// `is_bucketed` must name EXACTLY the fields `render_field` turns into a
    /// `"lo–hi"` range. Every filter surface branches on it to decide whether a
    /// typed value can be compared against the group key (discrete) or has to pin
    /// the underlying amount (bucketed) — a field that drifts out of sync here
    /// makes its filter silently match nothing, which is the bug this locks.
    /// Every field is populated so no arm can pass by rendering the `∅` sentinel.
    #[test]
    fn is_bucketed_agrees_with_render_field() {
        let mut f = fp();
        f.cu_price = Some(1_000);
        f.spendable_lamports_in = Some(500_000_000);
        f.first_slot_sell_sol = Some(0.75);
        let decimals = decimals_for(SOL_BUCKET_WIDTH);
        for field in GroupField::ALL {
            let rendered = render_field(&f, field, SOL_BUCKET_WIDTH.into(), decimals);
            assert_ne!(rendered, MISSING, "{field:?}: fixture must populate every field");
            // Exact mode must key on the amount itself — never a range, and never
            // the `∅` sentinel — for exactly the bucketed fields.
            let exact = render_field(&f, field, SolPrecision::Exact, 0);
            assert_ne!(exact, MISSING, "{field:?}: exact mode lost the value");
            assert!(!exact.contains('–'), "{field:?}: exact mode still ranged: {exact}");
            assert_eq!(
                exact != rendered,
                field.is_bucketed(),
                "{field:?}: only bucketed fields may differ between the two modes",
            );
            // The en-dash separator is the observable mark of a bucket label; no
            // discrete rendering (id, integer, bool, " | "-joined labels) contains it.
            assert_eq!(
                rendered.contains('–'),
                field.is_bucketed(),
                "{field:?}: is_bucketed()={} but render_field gave {rendered:?}",
                field.is_bucketed(),
            );
        }
    }

    #[test]
    fn exact_single_field_key() {
        let k = group_key(&fp(), &[GroupField::TokenProgramId], SOL_BUCKET_WIDTH);
        assert_eq!(k.0, vec![(GroupField::TokenProgramId, "Tokenkeg".to_string())]);
        assert_eq!(k.to_json(), json!({ "token_program_id": "Tokenkeg" }));
    }

    #[test]
    fn compound_key_in_selection_order() {
        // max_cost_lamports is a continuous SOL amount → binned (1_000_000_000
        // lamports = 1.0 SOL → the [1.0, 1.1) bucket); cu_limit is discrete → exact.
        let k = group_key(&fp(), &[GroupField::MaxCostLamports, GroupField::CuLimit], SOL_BUCKET_WIDTH);
        assert_eq!(
            k.to_json(),
            json!({ "max_cost_lamports": "1.0–1.1", "cu_limit": "200000" })
        );
    }

    #[test]
    fn missing_value_gets_sentinel_not_collision() {
        // cu_price is None → sentinel, distinct from a real "0".
        let missing = group_key(&fp(), &[GroupField::CuPrice], SOL_BUCKET_WIDTH);
        let mut z = fp();
        z.cu_price = Some(0);
        let zero = group_key(&z, &[GroupField::CuPrice], SOL_BUCKET_WIDTH);
        assert_ne!(missing, zero);
        assert_eq!(missing.0[0].1, MISSING);
    }

    #[test]
    fn empty_fields_is_single_all_group() {
        let a = group_key(&fp(), &[], SOL_BUCKET_WIDTH);
        let mut other = fp();
        other.token_program_id = Some("OtherProgram".into());
        let b = group_key(&other, &[], SOL_BUCKET_WIDTH);
        assert_eq!(a, b, "no grouping ⇒ every token shares the ALL key");
        assert_eq!(a.to_json(), json!({}));
    }

    #[test]
    fn labels_normalized_order_preserved_and_joined() {
        // exact on-chain sequence preserved — consecutive duplicates are kept
        assert_eq!(normalize_labels(&json!(["b", "a", "a"])), vec!["b", "a", "a"]);
        // Object shape used by some persisted token/trade rows.
        assert_eq!(
            normalize_labels(&json!({"instructions": ["b", "a", "a"]})),
            vec!["b", "a", "a"]
        );
        assert!(normalize_labels(&json!({"instructions": "nope"})).is_empty());
        let k = group_key(&fp(), &[GroupField::IxLabels], SOL_BUCKET_WIDTH);
        assert_eq!(k.0[0].1, "Pump.Fun: Create | System: Transfer");
    }

    #[test]
    fn first_slot_fields_round_trip_and_render() {
        // as_str / from_tag round-trip for the new trade-derived variants.
        for f in [GroupField::FirstSlotBuySol, GroupField::FirstSlotSellSol] {
            assert_eq!(GroupField::from_tag(f.as_str()), Some(f));
        }
        // Present value renders its bucket; None → sentinel (own group, no "0" collision).
        let buy = group_key(&fp(), &[GroupField::FirstSlotBuySol], SOL_BUCKET_WIDTH);
        assert_eq!(buy.0[0].1, "2.2–2.3"); // 2.25 ∈ [2.2, 2.3)
        let sell = group_key(&fp(), &[GroupField::FirstSlotSellSol], SOL_BUCKET_WIDTH);
        assert_eq!(sell.0[0].1, MISSING);
    }

    #[test]
    fn continuous_fields_bucket_into_ranges() {
        let label = |buy: f64| {
            let mut f = fp();
            f.initial_buy_sol = Some(buy);
            group_key(&f, &[GroupField::InitialBuySol], SOL_BUCKET_WIDTH).0[0].1.clone()
        };
        assert_eq!(label(2.34), "2.3–2.4", "mid-bucket");
        assert_eq!(label(0.3), "0.3–0.4", "on-edge → upper bucket (0.1 not f64-exact)");
        assert_eq!(label(0.0), "0.0–0.1");
        // Two nearby-but-distinct sums now share a group instead of being singletons.
        assert_eq!(label(1.51), label(1.55), "both land in [1.5, 1.6)");
        // None still forms its own sentinel group.
        let mut none = fp();
        none.initial_buy_sol = None;
        assert_eq!(group_key(&none, &[GroupField::InitialBuySol], SOL_BUCKET_WIDTH).0[0].1, MISSING);
    }

    #[test]
    fn per_run_width_changes_bucket_labels_and_decimals() {
        // The same value renders a different bucket label at a different per-run
        // width, and the decimals track the width (decimals_for) so the edge — always
        // a multiple of width — is rendered exactly (no rounding vs the dashboard SQL).
        let label = |buy: f64, width: f64| {
            let mut f = fp();
            f.initial_buy_sol = Some(buy);
            group_key(&f, &[GroupField::InitialBuySol], width).0[0].1.clone()
        };
        assert_eq!(label(2.34, 0.1), "2.3–2.4", "width 0.1 → 1 decimal");
        assert_eq!(label(2.34, 0.25), "2.25–2.50", "width 0.25 → 2 decimals, [2.25,2.50)");
        assert_eq!(label(2.34, 1.0), "2–3", "width 1.0 → 0 decimals");
        assert_eq!(label(0.62, 0.25), "0.50–0.75");
    }

    #[test]
    fn decimals_for_matches_width() {
        for (w, d) in [(1.0, 0), (5.0, 0), (0.5, 1), (0.1, 1), (0.25, 2), (0.05, 2)] {
            assert_eq!(decimals_for(w), d, "width {w}");
        }
    }

    #[test]
    fn same_bucket_agrees_with_label() {
        // The matcher's alloc-free membership test must agree with the label form
        // for every pair — both derive from bucket_index.
        let w = SOL_BUCKET_WIDTH;
        for a in [0.0, 0.05, 0.1, 0.3, 1.0, 1.05, 1.09, 1.1, 2.34, 8.0] {
            for b in [0.0, 0.05, 0.1, 0.3, 1.0, 1.05, 1.09, 1.1, 2.34, 8.0] {
                let via_label = bucket_sol_label(a, w, SOL_BUCKET_DECIMALS)
                    == bucket_sol_label(b, w, SOL_BUCKET_DECIMALS);
                assert_eq!(same_bucket(a, b, w), via_label, "a={a} b={b}");
            }
        }
        // On-edge lands in the upper bucket, matching bucket_sol_label's behavior.
        assert!(same_bucket(0.3, 0.35, w));
        assert!(!same_bucket(0.29, 0.3, w));
    }

    #[test]
    fn lamports_from_number_or_string() {
        let ix = json!({ "max_cost_lamports": 1_000_000_000u64, "spendable_lamports_in": "500" });
        assert_eq!(extract_lamports(Some(&ix), "max_cost_lamports"), Some(1_000_000_000));
        assert_eq!(extract_lamports(Some(&ix), "spendable_lamports_in"), Some(500));
        assert_eq!(extract_lamports(Some(&ix), "absent"), None);
        assert_eq!(extract_lamports(None, "max_cost_lamports"), None);
    }

}
