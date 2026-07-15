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
fn bucket_sol_label(v: f64, width: f64, decimals: usize) -> String {
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
    bucket_sol_label(lamports as f64 / 1_000_000_000.0, width, decimals)
}

/// Token-creation metadata used **only** for grouping — never read by any
/// `simulate()`. Carried on each corpus token so grouping is a pure in-memory
/// pass with no extra DB hit in the sweep loop.
#[derive(Clone, Debug, Default)]
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
    /// Creation instruction-label set, sorted + deduped for a stable key.
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

/// Collect an instruction-labels JSON array into a `Vec<String>`, preserving
/// exact on-chain order and count.
pub fn normalize_labels(labels: &Value) -> Vec<String> {
    labels
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
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
/// bucket `width` (the per-run partition width for the continuous SOL fields;
/// discrete fields ignore it). Pass [`SOL_BUCKET_WIDTH`] for the default.
pub fn group_key(fp: &TokenFingerprint, fields: &[GroupField], width: f64) -> GroupKey {
    // Derive decimals once from the width so every continuous field renders its
    // (width-multiple) edge exactly — the invariant that keeps the label byte-for-byte
    // identical to the dashboard SQL `to_char` (see `creation_stats_repo::sol_bucket_sql`).
    let decimals = decimals_for(width);
    GroupKey(fields.iter().map(|f| (*f, render_field(fp, *f, width, decimals))).collect())
}

/// Group-key string for one field. `None`/empty → [`MISSING`].
///
/// Discrete fields (program id, CU limit/price, cashback, ix-labels) render their
/// **exact** value. The continuous SOL-amount fields (initial buy, max cost,
/// spendable-in, first-slot buy/sell) are **binned** into `width`-wide ranges at
/// `decimals` = [`decimals_for`]`(width)` places — this is the single seam where a
/// value becomes a coarse, groupable key.
fn render_field(fp: &TokenFingerprint, f: GroupField, width: f64, decimals: usize) -> String {
    let opt = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| MISSING.to_string());
    let miss = || MISSING.to_string();
    match f {
        GroupField::TokenProgramId => {
            fp.token_program_id.clone().unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::CuLimit => opt(fp.cu_limit),
        GroupField::CuPrice => opt(fp.cu_price),
        GroupField::IsCashbackEnabled => fp.is_cashback_enabled.to_string(),
        // Continuous SOL amounts → binned SOL ranges. Lamports fields convert to SOL
        // first so the label reads in SOL (matching their "SOL cost"/"SOL in" name).
        GroupField::MaxCostLamports => {
            fp.max_cost_lamports.map(|l| bucket_lamports_as_sol(l, width, decimals)).unwrap_or_else(miss)
        }
        GroupField::SpendableLamportsIn => {
            fp.spendable_lamports_in.map(|l| bucket_lamports_as_sol(l, width, decimals)).unwrap_or_else(miss)
        }
        GroupField::InitialBuySol => fp
            .initial_buy_sol
            .map(|v| bucket_sol_label(v, width, decimals))
            .unwrap_or_else(miss),
        GroupField::FirstSlotBuySol => fp
            .first_slot_buy_sol
            .map(|v| bucket_sol_label(v, width, decimals))
            .unwrap_or_else(miss),
        GroupField::FirstSlotSellSol => fp
            .first_slot_sell_sol
            .map(|v| bucket_sol_label(v, width, decimals))
            .unwrap_or_else(miss),
        GroupField::IxLabels => {
            if fp.ix_labels.is_empty() {
                MISSING.to_string()
            } else {
                fp.ix_labels.join(" | ")
            }
        }
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
