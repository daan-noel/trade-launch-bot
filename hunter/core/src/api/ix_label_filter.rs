//! The instruction-label filter grammar - ONE parser, one in-RAM matcher and one
//! SQL fragment, shared by every token-table backend: the Tokens list (in-RAM
//! `TokenQuery::matches` and the `live` SQL page in `handlers::tokens::sql`), the
//! SQL-paged strategy tables (`strategy_repo`, via `FilterKind::IxLabels`) and the
//! in-memory evaluator (`table_eval`, via `ColKind::IxLabels`). The TS twin that
//! client-side tables run is `parseIxLabelFilter` / `ixLabelsMatchFilter`
//! (`frontend/src/shared/lib/ixLabels.ts`).
//!
//! * A JSON array (or `{"instructions": [...]}`) is an ordered, exact,
//!   case-insensitive sequence match: same length, element-wise equal.
//! * Anything else splits on commas/newlines into needles; a row passes when ANY
//!   label contains ANY needle (case-insensitive substring).
//!
//! A backend that resolves the `ix_labels` key to a plain text column instead
//! substring-matches the JSON text, so a pasted sequence or a comma list never
//! matches; one that does not resolve the key at all drops the filter and shows
//! every row. Route the key here.

use serde_json::Value;

use crate::storage::ix_labels_sql::ix_labels_elements_sql;

/// A parsed ix-label filter. Needles are lowercased at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IxLabelFilter {
    /// Empty / blank input: no constraint.
    None,
    /// Any label contains any needle.
    Text(Vec<String>),
    /// The label sequence equals the needles, in order.
    Json(Vec<String>),
}

/// A SQL predicate around ONE `text[]` bind: the caller writes
/// `{before}{placeholder}{after}` and binds `bind` at the placeholder, so the
/// positional-arg builder (`handlers::tokens::sql`) and `sqlx::QueryBuilder`
/// (`strategy_repo`) emit the same text.
pub struct IxLabelSql {
    pub before: String,
    pub bind: Vec<String>,
    pub after: &'static str,
}

/// Instruction labels of a stored `ix_labels` value (bare array or
/// `{instructions:[...]}`), lowercased. Non-string elements use their JSON text.
pub fn label_list(value: &Value) -> Vec<String> {
    let arr: &[Value] = match value {
        Value::Array(a) => a,
        Value::Object(o) => match o.get("instructions") {
            Some(Value::Array(a)) => a,
            _ => &[],
        },
        _ => &[],
    };
    arr.iter()
        .map(|v| match v {
            Value::String(s) => s.to_lowercase(),
            other => other.to_string().to_lowercase(),
        })
        .collect()
}

impl IxLabelFilter {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::None;
        }
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                let arr = match &parsed {
                    Value::Array(a) => Some(a),
                    Value::Object(o) => match o.get("instructions") {
                        Some(Value::Array(a)) => Some(a),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(a) = arr {
                    let needles: Vec<String> = a
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.trim().to_lowercase(),
                            other => other.to_string().trim().to_lowercase(),
                        })
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !needles.is_empty() {
                        return Self::Json(needles);
                    }
                }
            }
            // Not a label array: fall through to text mode.
        }
        let needles: Vec<String> = trimmed
            .split(['\n', ','])
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if needles.is_empty() {
            Self::None
        } else {
            Self::Text(needles)
        }
    }

    /// Does a stored `ix_labels` value pass? `None` passes everything.
    pub fn matches(&self, value: &Value) -> bool {
        match self {
            Self::None => true,
            Self::Json(needles) => label_list(value) == *needles,
            Self::Text(needles) => {
                let labels = label_list(value);
                needles.iter().any(|n| labels.iter().any(|l| l.contains(n.as_str())))
            }
        }
    }

    /// The predicate over the JSONB column `col` (both stored shapes), or `None`
    /// for [`IxLabelFilter::None`] (no clause).
    pub fn sql(&self, col: &str) -> Option<IxLabelSql> {
        let elems = ix_labels_elements_sql(col);
        match self {
            Self::None => None,
            Self::Text(needles) => Some(IxLabelSql {
                before: format!("EXISTS (SELECT 1 FROM {elems} AS x(e), unnest("),
                // LIKE metachars in a needle match literally.
                bind: needles
                    .iter()
                    .map(|n| n.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
                    .collect(),
                after: "::text[]) AS n(needle) WHERE LOWER(e) LIKE '%' || n.needle || '%' ESCAPE '\\')",
            }),
            Self::Json(needles) => Some(IxLabelSql {
                before: format!(
                    "(ARRAY(SELECT LOWER(e) FROM {elems} WITH ORDINALITY AS x(e, ord) ORDER BY ord) = "
                ),
                bind: needles.clone(),
                after: "::text[])",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels() -> Value {
        json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Create_v2", "Pump.Fun: BuyV2"])
    }

    #[test]
    fn json_is_ordered_exact_and_case_insensitive() {
        let f = IxLabelFilter::parse(
            r#"["compute budget: setcomputeunitlimit", "PUMP.FUN: CREATE_V2", "Pump.Fun: BuyV2"]"#,
        );
        assert!(matches!(f, IxLabelFilter::Json(_)));
        assert!(f.matches(&labels()));
        // The object wrapper matches the same way, on either side.
        assert!(f.matches(&json!({ "instructions": labels() })));
        // A prefix of the sequence is not the sequence.
        assert!(!IxLabelFilter::parse(r#"["Compute Budget: SetComputeUnitLimit"]"#).matches(&labels()));
        // Same labels, different order.
        assert!(!IxLabelFilter::parse(
            r#"["Pump.Fun: Create_v2", "Compute Budget: SetComputeUnitLimit", "Pump.Fun: BuyV2"]"#
        )
        .matches(&labels()));
    }

    #[test]
    fn pretty_printed_json_paste_is_json_mode() {
        // What the IX Labels cell copies (`JSON.stringify(arr, null, 2)`), with the
        // newlines a single-line input strips.
        let pasted = "[  \"Compute Budget: SetComputeUnitLimit\",  \"Pump.Fun: Create_v2\",  \"Pump.Fun: BuyV2\"]";
        assert!(IxLabelFilter::parse(pasted).matches(&labels()));
    }

    #[test]
    fn text_is_any_needle_in_any_label() {
        assert!(IxLabelFilter::parse("buyv2").matches(&labels()));
        assert!(IxLabelFilter::parse("nope, create_v2").matches(&labels()));
        assert!(!IxLabelFilter::parse("sell").matches(&labels()));
        assert!(!IxLabelFilter::parse("buy").matches(&Value::Null));
    }

    #[test]
    fn blank_or_empty_array_is_no_constraint_or_text() {
        assert_eq!(IxLabelFilter::parse("  "), IxLabelFilter::None);
        assert_eq!(IxLabelFilter::parse(" , \n "), IxLabelFilter::None);
        // `[]` is not a label array: it falls through to a text needle.
        assert_eq!(IxLabelFilter::parse("[]"), IxLabelFilter::Text(vec!["[]".into()]));
        assert!(IxLabelFilter::None.sql("t.ix_labels").is_none());
    }

    #[test]
    fn sql_binds_one_text_array() {
        let s = IxLabelFilter::parse("a_b, c%").sql("t.ix_labels").unwrap();
        assert_eq!(s.bind, vec!["a\\_b".to_string(), "c\\%".to_string()]);
        assert!(s.before.contains("t.ix_labels->'instructions'"));
        let s = IxLabelFilter::parse(r#"["A","B"]"#).sql("t.ix_labels").unwrap();
        assert_eq!(s.bind, vec!["a".to_string(), "b".to_string()]);
        assert!(s.before.contains("WITH ORDINALITY"));
    }
}
