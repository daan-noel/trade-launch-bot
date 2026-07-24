//! Postgres SQL fragments for reading `tokens.ix_labels` / `trades.ix_labels`.
//!
//! Persisted JSONB accepts two shapes (historical + current ingest):
//! * bare array — `["Pump.Fun: Create", "Pump.Fun: Buy"]`
//! * object wrapper — `{"instructions": ["Pump.Fun: Create", "Pump.Fun: Buy"]}`
//!
//! Every SQL path that unnests or counts labels must go through these helpers
//! so a shape change can't silently zero out fingerprint groups / filters.

/// JSONB array expression for an `ix_labels` column, unwrapping both shapes.
/// Non-array ⇒ `'[]'::jsonb` (empty, never NULL) so callers can `jsonb_array_*`
/// without a typeof guard of their own.
pub fn ix_labels_array_sql(col: &str) -> String {
    format!(
        "(CASE \
            WHEN jsonb_typeof({col}) = 'array' THEN {col} \
            WHEN jsonb_typeof({col}->'instructions') = 'array' THEN {col}->'instructions' \
            ELSE '[]'::jsonb END)"
    )
}

/// `jsonb_array_elements_text(...)` over [`ix_labels_array_sql`] — the set/sequence
/// source for filters and `" | "`-joined group keys.
pub fn ix_labels_elements_sql(col: &str) -> String {
    format!("jsonb_array_elements_text({})", ix_labels_array_sql(col))
}

/// Ordered exact-match predicate: the column's unwrapped label sequence equals
/// a bound `text[]` placeholder (`$n`). Case-sensitive, order-sensitive, keeps
/// duplicates — mirrors `hunter_engine::fingerprint::matches` on `ix_labels`.
pub fn ix_labels_ordered_eq_sql(col: &str, placeholder: &str) -> String {
    let elems = ix_labels_elements_sql(col);
    format!(
        "(ARRAY(SELECT e FROM {elems} WITH ORDINALITY AS x(e, ord) ORDER BY ord) = {placeholder})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_sql_unwraps_both_shapes() {
        let sql = ix_labels_array_sql("t.ix_labels");
        assert!(sql.contains("jsonb_typeof(t.ix_labels) = 'array'"));
        assert!(sql.contains("t.ix_labels->'instructions'"));
        assert!(sql.contains("'[]'::jsonb"));
    }

    #[test]
    fn ordered_eq_uses_elements_and_placeholder() {
        let sql = ix_labels_ordered_eq_sql("t.ix_labels", "$6");
        assert!(sql.contains("WITH ORDINALITY"));
        assert!(sql.contains("= $6"));
        assert!(sql.contains("t.ix_labels->'instructions'"));
    }
}
