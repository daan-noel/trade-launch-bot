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
//! v1 is **exact-value** grouping. Binning (numeric → bucket ranges) is a future
//! extension that lives entirely inside [`render_field`] (e.g. a `Bin(width)`
//! variant) — no engine, schema, or API change.

use serde::{Deserialize, Serialize};
use serde_json::Value;


/// Sentinel rendered for a missing (`None`/empty) field value, so tokens that
/// lack the field form their own group instead of colliding with `0`/`""`.
const MISSING: &str = "∅";

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

/// Compute the group key for one token's fingerprint under the chosen fields.
pub fn group_key(fp: &TokenFingerprint, fields: &[GroupField]) -> GroupKey {
    GroupKey(fields.iter().map(|f| (*f, render_field(fp, *f))).collect())
}

/// Canonical exact-value string for one field. `None`/empty → [`MISSING`].
/// The single seam for future binning (numeric fields → bucket labels here).
fn render_field(fp: &TokenFingerprint, f: GroupField) -> String {
    let opt = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| MISSING.to_string());
    match f {
        GroupField::TokenProgramId => {
            fp.token_program_id.clone().unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::CuLimit => opt(fp.cu_limit),
        GroupField::CuPrice => opt(fp.cu_price),
        GroupField::IsCashbackEnabled => fp.is_cashback_enabled.to_string(),
        GroupField::MaxCostLamports => opt(fp.max_cost_lamports),
        GroupField::SpendableLamportsIn => opt(fp.spendable_lamports_in),
        GroupField::InitialBuySol => {
            fp.initial_buy_sol.map(|v| format!("{v}")).unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::FirstSlotBuySol => {
            fp.first_slot_buy_sol.map(|v| format!("{v}")).unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::FirstSlotSellSol => {
            fp.first_slot_sell_sol.map(|v| format!("{v}")).unwrap_or_else(|| MISSING.to_string())
        }
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
        let k = group_key(&fp(), &[GroupField::TokenProgramId]);
        assert_eq!(k.0, vec![(GroupField::TokenProgramId, "Tokenkeg".to_string())]);
        assert_eq!(k.to_json(), json!({ "token_program_id": "Tokenkeg" }));
    }

    #[test]
    fn compound_key_in_selection_order() {
        let k = group_key(&fp(), &[GroupField::MaxCostLamports, GroupField::CuLimit]);
        assert_eq!(
            k.to_json(),
            json!({ "max_cost_lamports": "1000000000", "cu_limit": "200000" })
        );
    }

    #[test]
    fn missing_value_gets_sentinel_not_collision() {
        // cu_price is None → sentinel, distinct from a real "0".
        let missing = group_key(&fp(), &[GroupField::CuPrice]);
        let mut z = fp();
        z.cu_price = Some(0);
        let zero = group_key(&z, &[GroupField::CuPrice]);
        assert_ne!(missing, zero);
        assert_eq!(missing.0[0].1, MISSING);
    }

    #[test]
    fn empty_fields_is_single_all_group() {
        let a = group_key(&fp(), &[]);
        let mut other = fp();
        other.token_program_id = Some("OtherProgram".into());
        let b = group_key(&other, &[]);
        assert_eq!(a, b, "no grouping ⇒ every token shares the ALL key");
        assert_eq!(a.to_json(), json!({}));
    }

    #[test]
    fn labels_normalized_order_preserved_and_joined() {
        // exact on-chain sequence preserved — consecutive duplicates are kept
        assert_eq!(normalize_labels(&json!(["b", "a", "a"])), vec!["b", "a", "a"]);
        let k = group_key(&fp(), &[GroupField::IxLabels]);
        assert_eq!(k.0[0].1, "Pump.Fun: Create | System: Transfer");
    }

    #[test]
    fn first_slot_fields_round_trip_and_render() {
        // as_str / from_tag round-trip for the new trade-derived variants.
        for f in [GroupField::FirstSlotBuySol, GroupField::FirstSlotSellSol] {
            assert_eq!(GroupField::from_tag(f.as_str()), Some(f));
        }
        // Present value renders exactly; None → sentinel (own group, not a "0" collision).
        let buy = group_key(&fp(), &[GroupField::FirstSlotBuySol]);
        assert_eq!(buy.0[0].1, "2.25");
        let sell = group_key(&fp(), &[GroupField::FirstSlotSellSol]);
        assert_eq!(sell.0[0].1, MISSING);
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
