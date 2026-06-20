//! Strategy-agnostic token grouping.
//!
//! A sweep can partition its corpus by a **compound fingerprint key** — the
//! exact values of one or more token-creation fields the user picks on the page
//! (creator, CU settings, the creation-instruction `max_sol_cost` /
//! `spendable_sol_in`, the instruction-label set, …). Each surviving group is
//! swept independently so the UI can answer "for tokens with *this* fingerprint,
//! which param combo is best?".
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
    pub creator_wallet: String,
    pub token_program_id: Option<String>,
    pub initial_buy_sol: Option<f64>,
    pub cu_limit: Option<i64>,
    pub cu_price: Option<i64>,
    pub is_cashback_enabled: bool,
    /// Creation-instruction `max_sol_cost`, in lamports.
    pub max_sol_cost: Option<i64>,
    /// Creation-instruction `spendable_sol_in`, in lamports.
    pub spendable_sol_in: Option<i64>,
    /// Creation instruction-label set, sorted + deduped for a stable key.
    pub ix_labels: Vec<String>,
}

/// Read a lamports value (u64 number or numeric string) from a creation
/// instruction-args object. Mirrors the live entry path's `instruction_arg_as_sol`
/// reader (minus the SOL conversion — grouping keys on raw lamports).
pub fn extract_lamports(instruction: Option<&Value>, key: &str) -> Option<i64> {
    instruction?.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
}

/// Dedup an instruction-labels JSON array, preserving on-chain order.
pub fn normalize_labels(labels: &Value) -> Vec<String> {
    let v: Vec<String> = labels
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    normalize_label_vec(v)
}

/// Dedup a label `Vec`, preserving on-chain order. Shared by [`normalize_labels`]
/// and the grouped-sweep ix-labels corpus filter (the filter must normalize
/// identically to the fingerprint so the `==` comparison is consistent).
pub fn normalize_label_vec(mut v: Vec<String>) -> Vec<String> {
    v.dedup();
    v
}

/// One selectable grouping field. Serde snake_case tags match the `group_by`
/// array the frontend sends and the keys stored in a [`GroupKey`]'s JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupField {
    CreatorWallet,
    TokenProgramId,
    CuLimit,
    CuPrice,
    IsCashbackEnabled,
    MaxSolCost,
    SpendableSolIn,
    InitialBuySol,
    IxLabels,
}

impl GroupField {
    /// Stable key used in the `GroupKey` JSON object (matches the serde tag).
    pub fn as_str(self) -> &'static str {
        match self {
            GroupField::CreatorWallet => "creator_wallet",
            GroupField::TokenProgramId => "token_program_id",
            GroupField::CuLimit => "cu_limit",
            GroupField::CuPrice => "cu_price",
            GroupField::IsCashbackEnabled => "is_cashback_enabled",
            GroupField::MaxSolCost => "max_sol_cost",
            GroupField::SpendableSolIn => "spendable_sol_in",
            GroupField::InitialBuySol => "initial_buy_sol",
            GroupField::IxLabels => "ix_labels",
        }
    }
}

/// A compound group key: the chosen fields' exact values in selection order.
/// Stringified so any field type collapses to one hashable/comparable key and
/// round-trips to JSON for storage + the UI. An empty `Vec` is the single "ALL"
/// group (no grouping selected ⇒ one global sweep, same as the legacy table).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupKey(pub Vec<(GroupField, String)>);

impl GroupKey {
    /// `{"creator_wallet":"4f3a…","max_sol_cost":"12345"}` — stored on the group
    /// row and rendered as chips by the page. Empty key ⇒ `{}`.
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
        GroupField::CreatorWallet => fp.creator_wallet.clone(),
        GroupField::TokenProgramId => {
            fp.token_program_id.clone().unwrap_or_else(|| MISSING.to_string())
        }
        GroupField::CuLimit => opt(fp.cu_limit),
        GroupField::CuPrice => opt(fp.cu_price),
        GroupField::IsCashbackEnabled => fp.is_cashback_enabled.to_string(),
        GroupField::MaxSolCost => opt(fp.max_sol_cost),
        GroupField::SpendableSolIn => opt(fp.spendable_sol_in),
        GroupField::InitialBuySol => {
            fp.initial_buy_sol.map(|v| format!("{v}")).unwrap_or_else(|| MISSING.to_string())
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
            creator_wallet: "devA".into(),
            token_program_id: Some("Tokenkeg".into()),
            initial_buy_sol: Some(1.5),
            cu_limit: Some(200_000),
            cu_price: None,
            is_cashback_enabled: true,
            max_sol_cost: Some(1_000_000_000),
            spendable_sol_in: None,
            ix_labels: vec!["Pump.Fun: Create".into(), "System: Transfer".into()],
        }
    }

    #[test]
    fn exact_single_field_key() {
        let k = group_key(&fp(), &[GroupField::CreatorWallet]);
        assert_eq!(k.0, vec![(GroupField::CreatorWallet, "devA".to_string())]);
        assert_eq!(k.to_json(), json!({ "creator_wallet": "devA" }));
    }

    #[test]
    fn compound_key_in_selection_order() {
        let k = group_key(&fp(), &[GroupField::MaxSolCost, GroupField::CuLimit]);
        assert_eq!(
            k.to_json(),
            json!({ "max_sol_cost": "1000000000", "cu_limit": "200000" })
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
        other.creator_wallet = "devB".into();
        let b = group_key(&other, &[]);
        assert_eq!(a, b, "no grouping ⇒ every token shares the ALL key");
        assert_eq!(a.to_json(), json!({}));
    }

    #[test]
    fn labels_normalized_order_preserved_and_joined() {
        // dedup removes consecutive duplicates but preserves on-chain order
        assert_eq!(normalize_labels(&json!(["b", "a", "a"])), vec!["b", "a"]);
        let k = group_key(&fp(), &[GroupField::IxLabels]);
        assert_eq!(k.0[0].1, "Pump.Fun: Create | System: Transfer");
    }

    #[test]
    fn lamports_from_number_or_string() {
        let ix = json!({ "max_sol_cost": 1_000_000_000u64, "spendable_sol_in": "500" });
        assert_eq!(extract_lamports(Some(&ix), "max_sol_cost"), Some(1_000_000_000));
        assert_eq!(extract_lamports(Some(&ix), "spendable_sol_in"), Some(500));
        assert_eq!(extract_lamports(Some(&ix), "absent"), None);
        assert_eq!(extract_lamports(None, "max_sol_cost"), None);
    }
}
