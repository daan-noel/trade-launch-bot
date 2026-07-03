//! SQL execution backend for the token list (the `live` bin's `/api/tokens`).
//!
//! `lab` runs the in-RAM engine in `tokens.rs` (`TokenQuery::matches` / `sort_refs`)
//! over a full in-RAM snapshot; this module is a **second backend for the same
//! grammar**: it turns a parsed [`TokenQuery`] into a parameterized `WHERE` +
//! `ORDER BY` fragment over `tokens t LEFT JOIN tokens_info i`, so `live` can page
//! the whole token universe from Postgres without holding it in RAM (the live cache
//! stays tracking-only).
//!
//! **Parity is the contract.** The per-column sort/filter grammar is single-sourced
//! in the `tokens.rs` column registry (`build_registry`): this module reads the SQL
//! side of each column via `sort_sql_expr` / `col_filter_number_sql` /
//! `col_filter_text_sql`, while the in-RAM engine reads the `TokenSummary` accessor
//! side of the SAME registry entry — so the two can't drift by construction. The
//! global `f_*` filter clauses below still mirror `TokenQuery::matches` directly.
//! The DB-backed `token_repo::parity_tests` (auto-runs with `DATABASE_URL`) proves
//! the two engines return identical ordered mint lists.
//!
//! Injection safety: user values are NEVER interpolated. Only fixed, code-chosen
//! column expressions are written into the SQL string; every `q` value is pushed as
//! a positional bind (`$n`) via [`SqlArgs`].

use chrono::{DateTime, Utc};

use crate::storage::token_enrichment::MARKET_CAP_SQL;
use super::tokens::{
    ath_fep_sql_expr, col_filter_number_sql, col_filter_text_sql, cur_fep_sql_expr,
    is_numeric_col, is_swing_sort_col, parse_dt_public, parse_numeric_predicate_public, sort_sql_expr,
    NumPredPublic, TokenQuery,
};

/// A single positional bind argument. Kept as a typed enum so the repo layer binds
/// each with the right sqlx type (no stringly-typed coercion at the DB boundary).
#[derive(Debug, Clone)]
pub enum SqlArg {
    Str(String),
    F64(f64),
    I64(i64),
    Ts(DateTime<Utc>),
    Bool(bool),
    /// Ordered lowercase label list for ix-label JSON set-equality.
    StrArray(Vec<String>),
}

/// Accumulates positional binds while the WHERE/ORDER fragment is built, so `$n`
/// placeholders and the bind vec stay in lockstep.
#[derive(Default)]
pub struct SqlArgs {
    args: Vec<SqlArg>,
}

impl SqlArgs {
    /// Push a bind and return its `$n` placeholder (1-based, matching sqlx).
    fn push(&mut self, arg: SqlArg) -> String {
        self.args.push(arg);
        format!("${}", self.args.len())
    }

    pub fn into_vec(self) -> Vec<SqlArg> {
        self.args
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

/// The built query fragments: a `WHERE`-body (already ANDed, no leading `WHERE`)
/// and an `ORDER BY`-body (no leading `ORDER BY`), plus the positional binds they
/// reference in order.
pub struct BuiltQuery {
    /// The predicate body (`TRUE` when no filters are active). Callers wrap it as
    /// `WHERE {where_sql}`.
    pub where_sql: String,
    /// The order body. Callers wrap it as `ORDER BY {order_sql}`.
    pub order_sql: String,
    pub args: Vec<SqlArg>,
}

/// Build the `WHERE` + `ORDER BY` fragments for a parsed token query. `now` seeds
/// the lifetime stale-guard (mirrors the in-RAM `lifetime_minutes` "still trading ⇒
/// exempt" rule).
pub fn build_where_and_order(q: &TokenQuery, now: DateTime<Utc>) -> BuiltQuery {
    let mut a = SqlArgs::default();
    let mut clauses: Vec<String> = Vec::new();

    // --- Identity (text_match: case-insensitive substring) ---
    for (expr, key) in [
        ("t.symbol", "symbol"),
        ("t.name", "name"),
        ("t.mint_address", "mint"),
        ("t.creator_wallet", "creator"),
        ("t.creation_tx_signature", "create_tx"),
    ] {
        if let Some(v) = q.f_get(key) {
            clauses.push(like_ci(expr, v, &mut a));
        }
    }

    // --- Time (date_in_range over an Option<timestamptz>) ---
    push_date_range(&mut clauses, "t.created_at", q.f_get("created_from"), q.f_get("created_to"), &mut a);
    push_date_range(&mut clauses, "i.last_trade_at", q.f_get("last_trade_from"), q.f_get("last_trade_to"), &mut a);
    push_date_range(&mut clauses, "i.ath_timestamp", q.f_get("ath_from"), q.f_get("ath_to"), &mut a);

    // --- Lifetime (minutes; dead-only stale guard) ---
    // Only apply when the token is NOT still-trading (last_trade older than the
    // stale window). A row that IS still trading is exempt (passes regardless).
    let life_min = q.f_get("life_min");
    let life_max = q.f_get("life_max");
    if life_min.is_some() || life_max.is_some() {
        // lifetime minutes = COALESCE(lifetime_secs, EPOCH(last_trade - created))/60
        let life_expr = "(COALESCE(i.lifetime_secs, EXTRACT(EPOCH FROM (i.last_trade_at - t.created_at)))/60.0)";
        // Stale guard: exempt (pass) rows whose last_trade is within LIFETIME_STALE_MS
        // of `now`, or that have no last_trade at all (None ⇒ exempt in-RAM too).
        let now_ph = a.push(SqlArg::Ts(now));
        let stale_guard = format!(
            "(i.last_trade_at IS NULL OR i.last_trade_at >= {now_ph} - interval '1 hour')"
        );
        let mut range = Vec::new();
        if let Some(v) = life_min {
            if let Ok(n) = v.parse::<f64>() {
                let ph = a.push(SqlArg::F64(n));
                range.push(format!("{life_expr} >= {ph}"));
            }
        }
        if let Some(v) = life_max {
            if let Ok(n) = v.parse::<f64>() {
                let ph = a.push(SqlArg::F64(n));
                range.push(format!("{life_expr} <= {ph}"));
            }
        }
        if !range.is_empty() {
            // Pass if exempt (still trading / no trade) OR the lifetime is in range.
            clauses.push(format!("({stale_guard} OR ({}))", range.join(" AND ")));
        }
    }

    // --- Performance (opt_f64: NULL fails when a bound is set) ---
    push_opt_range(&mut clauses, ath_fep_sql_expr(), q.f_get("ath_fep_min"), q.f_get("ath_fep_max"), &mut a);
    push_opt_range(&mut clauses, cur_fep_sql_expr(), q.f_get("cur_fep_min"), q.f_get("cur_fep_max"), &mut a);
    push_opt_range(&mut clauses, "i.ath_price", q.f_get("ath_price_min"), q.f_get("ath_price_max"), &mut a);
    push_opt_range(&mut clauses, "i.current_price", q.f_get("price_min"), q.f_get("price_max"), &mut a);

    // --- Market ---
    // volume / trade_count / ix_count use range_f64 (present-or-0). volume &
    // trade_count are NOT NULL in tokens_info, but the LEFT JOIN can null them for a
    // token with no info row → COALESCE to 0 to match the in-RAM `.unwrap_or(0.0)`.
    push_range(&mut clauses, "COALESCE(i.volume_sol, 0)", q.f_get("volume_min"), q.f_get("volume_max"), &mut a);
    push_opt_range(&mut clauses, "i.first_slot_buy_lamports::float8/1e9", q.f_get("first_slot_buy_min"), q.f_get("first_slot_buy_max"), &mut a);
    push_opt_range(&mut clauses, "i.first_slot_sell_lamports::float8/1e9", q.f_get("first_slot_sell_min"), q.f_get("first_slot_sell_max"), &mut a);
    push_opt_range(&mut clauses, MARKET_CAP_SQL, q.f_get("mcap_min"), q.f_get("mcap_max"), &mut a);
    push_range(&mut clauses, "COALESCE(i.trade_count, 0)", q.f_get("trades_min"), q.f_get("trades_max"), &mut a);
    // initial_buy_sol stored lamports; model is human SOL → /1e9.
    push_opt_range(&mut clauses, "t.initial_buy_lamports::float8/1e9", q.f_get("init_buy_min"), q.f_get("init_buy_max"), &mut a);
    push_opt_range(&mut clauses, "t.initial_supply_token::float8", q.f_get("init_supply_min"), q.f_get("init_supply_max"), &mut a);
    // token_amount / min_tokens_out from the buy-ix JSON (raw units, no /1e9).
    push_opt_range(&mut clauses, &buy_arg_expr("token_amount"), q.f_get("token_amount_min"), q.f_get("token_amount_max"), &mut a);
    // max_cost_lamports / spendable_lamports_in from the buy-ix JSON, lamports → SOL.
    push_opt_range(&mut clauses, &format!("({}/1e9)", buy_arg_expr("max_cost_lamports")), q.f_get("max_cost_lamports_min"), q.f_get("max_cost_lamports_max"), &mut a);
    push_opt_range(&mut clauses, &format!("({}/1e9)", buy_arg_expr("spendable_lamports_in")), q.f_get("spendable_lamports_in_min"), q.f_get("spendable_lamports_in_max"), &mut a);
    push_opt_range(&mut clauses, &buy_arg_expr("min_tokens_out"), q.f_get("min_tokens_out_min"), q.f_get("min_tokens_out_max"), &mut a);

    // --- Technical ---
    push_opt_range(&mut clauses, "t.cu_limit::float8", q.f_get("cu_limit_min"), q.f_get("cu_limit_max"), &mut a);
    push_opt_range(&mut clauses, "t.cu_price::float8", q.f_get("cu_price_min"), q.f_get("cu_price_max"), &mut a);
    // ix_count = jsonb array length of ix_labels (handles {instructions:[…]} shape).
    push_range(&mut clauses, ix_count_expr(), q.f_get("ix_count_min"), q.f_get("ix_count_max"), &mut a);
    if let Some(raw) = q.f_get("ix_label") {
        if let Some(c) = ix_label_clause(raw, &mut a) {
            clauses.push(c);
        }
    }

    // --- Flags (tri_match) ---
    push_tri(&mut clauses, "COALESCE(i.is_migrated, false)", q.f_get("migrated"));
    push_tri(&mut clauses, "COALESCE(i.is_dead, false)", q.f_get("dead"));
    push_tri(&mut clauses, "t.is_mayhem_mode", q.f_get("mayhem"));
    push_tri(&mut clauses, "t.is_cashback_enabled", q.f_get("cashback"));

    // --- Global search (full parity: text + date + numeric substrings) ---
    let search = q.search_str();
    if !search.is_empty() {
        clauses.push(search_clause(search, &mut a));
    }

    // --- Per-column filters (numeric grammar or substring) ---
    for (key, expr) in q.col_filters_slice() {
        if let Some(c) = col_filter_clause(key, expr, &mut a) {
            clauses.push(c);
        }
    }

    let where_sql = if clauses.is_empty() {
        "TRUE".to_string()
    } else {
        clauses.join(" AND ")
    };

    let order_sql = build_order(q);

    BuiltQuery { where_sql, order_sql, args: a.into_vec() }
}

// ---------------------------------------------------------------------------
// Column-expression helpers (buy-ix JSON, ix labels)
// ---------------------------------------------------------------------------

/// SQL numeric expression reading a buy-instruction arg from `initial_buy_instruction`
/// JSONB (mirrors `extract_buy_arg_u64`). Non-numeric / absent ⇒ NULL.
fn buy_arg_expr(field: &str) -> String {
    // `->>` yields text; cast to float8 only when it looks numeric so a stray
    // string doesn't error the whole query (mirrors `as_u64()` returning None).
    format!("(CASE WHEN t.initial_buy_instruction->>'{field}' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'{field}')::float8 END)")
}

/// jsonb array length of the ix_labels, handling both the bare-array and the
/// `{instructions:[…]}` object shape; non-array ⇒ 0.
fn ix_count_expr() -> &'static str {
    "COALESCE(jsonb_array_length(CASE \
        WHEN jsonb_typeof(t.ix_labels) = 'array' THEN t.ix_labels \
        WHEN jsonb_typeof(t.ix_labels->'instructions') = 'array' THEN t.ix_labels->'instructions' \
        ELSE '[]'::jsonb END), 0)"
}

/// Lowercase text elements of ix_labels (both shapes), as a SQL set expression
/// usable inside EXISTS / array_agg.
fn ix_labels_elements_sql() -> &'static str {
    "jsonb_array_elements_text(CASE \
        WHEN jsonb_typeof(t.ix_labels) = 'array' THEN t.ix_labels \
        WHEN jsonb_typeof(t.ix_labels->'instructions') = 'array' THEN t.ix_labels->'instructions' \
        ELSE '[]'::jsonb END)"
}

// ---------------------------------------------------------------------------
// Clause builders
// ---------------------------------------------------------------------------

/// `LOWER(expr) LIKE '%' || LOWER($n) || '%'` — case-insensitive substring, the
/// value bound (never interpolated). `%`/`_` in the needle are treated literally
/// via an ESCAPE guard so a mint/creator string with those chars still matches.
fn like_ci(expr: &str, needle: &str, a: &mut SqlArgs) -> String {
    // Mirror text_match: trim + lowercase the needle. Escape LIKE metachars.
    let escaped = needle.trim().to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let ph = a.push(SqlArg::Str(escaped));
    format!("LOWER({expr}) LIKE '%' || {ph} || '%' ESCAPE '\\'")
}

/// date_in_range: pass when neither bound is set OR the value is a timestamp in the
/// [from, to] range. A NULL value fails when any bound is set (mirrors the in-RAM
/// `None => return false`).
fn push_date_range(
    clauses: &mut Vec<String>,
    expr: &str,
    from: Option<&str>,
    to: Option<&str>,
    a: &mut SqlArgs,
) {
    let lo = from.and_then(parse_dt_public);
    let hi = to.and_then(parse_dt_public);
    if lo.is_none() && hi.is_none() {
        return;
    }
    let mut parts = Vec::new();
    parts.push(format!("{expr} IS NOT NULL"));
    if let Some(f) = lo {
        let ph = a.push(SqlArg::Ts(f));
        parts.push(format!("{expr} >= {ph}"));
    }
    if let Some(u) = hi {
        let ph = a.push(SqlArg::Ts(u));
        parts.push(format!("{expr} <= {ph}"));
    }
    clauses.push(format!("({})", parts.join(" AND ")));
}

/// range_f64 over a NON-NULL numeric expression: a bound clamps, an absent bound is
/// open. Both bounds absent ⇒ no clause. Unparseable bound ⇒ that side is ignored
/// (mirrors the in-RAM `if let Ok(v) = min.parse()` — a bad bound is a no-op).
fn push_range(
    clauses: &mut Vec<String>,
    expr: &str,
    min: Option<&str>,
    max: Option<&str>,
    a: &mut SqlArgs,
) {
    let lo = min.and_then(|s| s.parse::<f64>().ok());
    let hi = max.and_then(|s| s.parse::<f64>().ok());
    let mut parts = Vec::new();
    if let Some(v) = lo {
        let ph = a.push(SqlArg::F64(v));
        parts.push(format!("{expr} >= {ph}"));
    }
    if let Some(v) = hi {
        let ph = a.push(SqlArg::F64(v));
        parts.push(format!("{expr} <= {ph}"));
    }
    if !parts.is_empty() {
        clauses.push(format!("({})", parts.join(" AND ")));
    }
}

/// opt_f64 over a NULLABLE numeric expression: both bounds absent ⇒ no clause; else
/// the value must be non-null AND in range (NULL fails).
fn push_opt_range(
    clauses: &mut Vec<String>,
    expr: &str,
    min: Option<&str>,
    max: Option<&str>,
    a: &mut SqlArgs,
) {
    let lo = min.and_then(|s| s.parse::<f64>().ok());
    let hi = max.and_then(|s| s.parse::<f64>().ok());
    // Mirror opt_f64: it only short-circuits to "pass" when BOTH raw strings are
    // empty. A present-but-unparseable bound still forces the None→false path.
    let min_active = min.map(|s| !s.is_empty()).unwrap_or(false);
    let max_active = max.map(|s| !s.is_empty()).unwrap_or(false);
    if !min_active && !max_active {
        return;
    }
    let mut parts = vec![format!("{expr} IS NOT NULL")];
    if let Some(v) = lo {
        let ph = a.push(SqlArg::F64(v));
        parts.push(format!("{expr} >= {ph}"));
    }
    if let Some(v) = hi {
        let ph = a.push(SqlArg::F64(v));
        parts.push(format!("{expr} <= {ph}"));
    }
    clauses.push(format!("({})", parts.join(" AND ")));
}

/// tri_match: "yes" ⇒ expr, "no" ⇒ NOT expr, anything else ⇒ no clause. The flag
/// values are compile-time constants, not user data, so no bind is needed.
fn push_tri(clauses: &mut Vec<String>, expr: &str, tri: Option<&str>) {
    match tri {
        Some("yes") => clauses.push(format!("{expr} = true")),
        Some("no") => clauses.push(format!("{expr} = false")),
        _ => {}
    }
}

/// Global-search clause — deliberately narrowed to `mint` + `symbol` only (locked
/// decision), mirroring the in-RAM `search_match`'s field set exactly. Dropping the
/// old date/numeric substring matching also removes the `float8::text` vs Rust
/// `to_string` formatting drift the two engines used to disagree on.
fn search_clause(raw: &str, a: &mut SqlArgs) -> String {
    let needle = raw.trim().to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let ph = a.push(SqlArg::Str(needle));
    let ors: Vec<String> = ["t.mint_address", "t.symbol"]
        .iter()
        .map(|c| format!("LOWER({c}) LIKE '%' || {ph} || '%' ESCAPE '\\'"))
        .collect();
    format!("({})", ors.join(" OR "))
}

/// Per-column `cf` filter: numeric grammar on numeric columns, else substring.
fn col_filter_clause(key: &str, expr: &str, a: &mut SqlArgs) -> Option<String> {
    let text = expr.trim();
    if text.is_empty() {
        return None;
    }
    if is_numeric_col(key) {
        if let Some(pred) = parse_numeric_predicate_public(text) {
            let col = col_filter_number_sql(key)?;
            return Some(numeric_pred_clause(&col, &pred, a));
        }
    }
    // Substring against the column's text projection.
    let col = col_filter_text_sql(key)?;
    let needle = text.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let ph = a.push(SqlArg::Str(needle));
    Some(format!("LOWER({col}) LIKE '%' || {ph} || '%' ESCAPE '\\'"))
}

/// A parsed numeric predicate → SQL over a NULLABLE numeric column expression. A
/// NULL column fails every predicate (mirrors `col_filter_number` None ⇒ false).
fn numeric_pred_clause(col: &str, pred: &NumPredPublic, a: &mut SqlArgs) -> String {
    let cmp = |op: &str, v: f64, a: &mut SqlArgs| {
        let ph = a.push(SqlArg::F64(v));
        format!("({col} IS NOT NULL AND {col} {op} {ph})")
    };
    match *pred {
        NumPredPublic::Range(lo, hi) => {
            let plo = a.push(SqlArg::F64(lo));
            let phi = a.push(SqlArg::F64(hi));
            format!("({col} IS NOT NULL AND {col} >= {plo} AND {col} <= {phi})")
        }
        NumPredPublic::Gt(v) => cmp(">", v, a),
        NumPredPublic::Ge(v) => cmp(">=", v, a),
        NumPredPublic::Lt(v) => cmp("<", v, a),
        NumPredPublic::Le(v) => cmp("<=", v, a),
        NumPredPublic::Ne(v) => cmp("!=", v, a),
        NumPredPublic::Eq(v) => cmp("=", v, a),
    }
}

/// ix_label filter → SQL (both text and JSON set-equality modes). Returns None when
/// the filter parses to nothing (matches `IxFilter::None` ⇒ no clause / pass-all).
fn ix_label_clause(raw: &str, a: &mut SqlArgs) -> Option<String> {
    use super::tokens::{parse_ix_label_filter_public, IxFilterPublic};
    match parse_ix_label_filter_public(raw) {
        IxFilterPublic::None => None,
        IxFilterPublic::Text(needles) => {
            // Any label contains any needle.
            let elems = ix_labels_elements_sql();
            let ors: Vec<String> = needles
                .iter()
                .map(|n| {
                    let esc = n.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
                    let ph = a.push(SqlArg::Str(esc));
                    format!("LOWER(e) LIKE '%' || {ph} || '%' ESCAPE '\\'")
                })
                .collect();
            Some(format!(
                "EXISTS (SELECT 1 FROM {elems} AS e WHERE {})",
                ors.join(" OR ")
            ))
        }
        IxFilterPublic::Json(needles) => {
            // Exact set-equality, elementwise + same length (in-RAM compares zipped,
            // so order matters): the ordered lowercase label array must equal the
            // ordered lowercase needle array.
            let elems = ix_labels_elements_sql();
            let lowered: Vec<String> = needles.iter().map(|n| n.to_lowercase()).collect();
            let ph = a.push(SqlArg::StrArray(lowered));
            Some(format!(
                "(ARRAY(SELECT LOWER(e) FROM {elems} WITH ORDINALITY AS x(e, ord) ORDER BY ord) = {ph})"
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// ORDER BY
// ---------------------------------------------------------------------------

/// Build the ORDER BY body. Each sort level maps to its column expression with an
/// explicit `NULLS LAST` (so nulls sort last regardless of direction, matching
/// `cmp_keys`). Swing-chain columns have no DB column ⇒ dropped (default order).
/// A final `t.mint_address ASC` is the stable tiebreak. Empty ⇒ newest-first.
fn build_order(q: &TokenQuery) -> String {
    let mut terms: Vec<String> = Vec::new();
    for (col, desc) in q.sort_levels() {
        if is_swing_sort_col(col) {
            continue; // no DB source on the live bin → fall through to default order
        }
        if let Some((expr, is_text)) = sort_sql_expr(col) {
            let dir = if *desc { "DESC" } else { "ASC" };
            // String sorts are case-insensitive (matches cmp_keys LOWER compare).
            let keyed = if is_text { format!("LOWER({expr})") } else { expr.to_string() };
            terms.push(format!("{keyed} {dir} NULLS LAST"));
        }
    }
    if terms.is_empty() {
        // Snapshot default: newest-first with a deterministic tiebreak.
        return "t.created_at DESC, t.mint_address DESC".to_string();
    }
    terms.push("t.mint_address ASC".to_string());
    terms.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handlers::tokens::{PaginationParams, TokenQuery};

    /// Build a `TokenQuery` from a JSON object of the (flat) `PaginationParams`
    /// fields — every field is `Option<String>` / primitive, so JSON round-trips
    /// cleanly and we avoid hand-constructing the ~60-field struct.
    fn query(json: serde_json::Value) -> TokenQuery {
        let params: PaginationParams = serde_json::from_value(json).expect("params");
        TokenQuery::from_params(&params)
    }

    fn now() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn no_filters_is_true_with_default_order() {
        let b = build_where_and_order(&query(serde_json::json!({})), now());
        assert_eq!(b.where_sql, "TRUE");
        assert_eq!(b.order_sql, "t.created_at DESC, t.mint_address DESC");
        assert!(b.args.is_empty());
    }

    #[test]
    fn text_filter_binds_lowercased_needle() {
        let b = build_where_and_order(&query(serde_json::json!({"f_symbol": "BONK"})), now());
        assert!(b.where_sql.contains("LOWER(t.symbol) LIKE '%' || $1 || '%'"));
        assert_eq!(b.args.len(), 1);
        match &b.args[0] {
            SqlArg::Str(s) => assert_eq!(s, "bonk"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn numeric_opt_range_requires_non_null() {
        let b = build_where_and_order(
            &query(serde_json::json!({"f_ath_price_min": "1.5", "f_ath_price_max": "9"})),
            now(),
        );
        assert!(b.where_sql.contains("i.ath_price IS NOT NULL"));
        assert!(b.where_sql.contains("i.ath_price >= $1"));
        assert!(b.where_sql.contains("i.ath_price <= $2"));
        assert!(matches!(b.args[0], SqlArg::F64(v) if (v - 1.5).abs() < 1e-9));
        assert!(matches!(b.args[1], SqlArg::F64(v) if (v - 9.0).abs() < 1e-9));
    }

    #[test]
    fn tri_flag_emits_no_bind() {
        let b = build_where_and_order(&query(serde_json::json!({"f_migrated": "yes"})), now());
        assert!(b.where_sql.contains("COALESCE(i.is_migrated, false) = true"));
        assert!(b.args.is_empty(), "tri-flags are constant, no bind");
    }

    #[test]
    fn tri_flag_no_means_false() {
        let b = build_where_and_order(&query(serde_json::json!({"f_dead": "no"})), now());
        assert!(b.where_sql.contains("COALESCE(i.is_dead, false) = false"));
    }

    #[test]
    fn multi_key_sort_maps_columns_with_nulls_last_and_tiebreak() {
        let b = build_where_and_order(
            &query(serde_json::json!({"sort": "trade_count:desc,symbol:asc"})),
            now(),
        );
        // trade_count numeric desc, symbol case-insensitive asc, then mint tiebreak.
        assert!(b.order_sql.contains("COALESCE(i.trade_count, 0) DESC NULLS LAST"));
        assert!(b.order_sql.contains("LOWER(t.symbol) ASC NULLS LAST"));
        assert!(b.order_sql.ends_with("t.mint_address ASC"));
    }

    #[test]
    fn swing_sort_column_is_dropped_to_default_order() {
        // No DB source for chain columns on live → falls back to default order.
        let b = build_where_and_order(&query(serde_json::json!({"sort": "swing_pairs:desc"})), now());
        assert_eq!(b.order_sql, "t.created_at DESC, t.mint_address DESC");
    }

    #[test]
    fn date_range_parses_and_binds_timestamps() {
        let b = build_where_and_order(
            &query(serde_json::json!({"f_created_from": "2026-01-01T00:00:00"})),
            now(),
        );
        assert!(b.where_sql.contains("t.created_at IS NOT NULL"));
        assert!(b.where_sql.contains("t.created_at >= $1"));
        assert!(matches!(b.args[0], SqlArg::Ts(_)));
    }

    #[test]
    fn per_column_numeric_predicate_translates_operator() {
        let b = build_where_and_order(&query(serde_json::json!({"cf": "trade_count:>=100"})), now());
        assert!(b.where_sql.contains("COALESCE(i.trade_count, 0) IS NOT NULL"));
        assert!(b.where_sql.contains(">= $1"));
        assert!(matches!(b.args[0], SqlArg::F64(v) if (v - 100.0).abs() < 1e-9));
    }

    #[test]
    fn ix_label_text_mode_builds_exists() {
        let b = build_where_and_order(&query(serde_json::json!({"f_ix_label": "buy"})), now());
        assert!(b.where_sql.contains("EXISTS (SELECT 1 FROM"));
        assert!(b.where_sql.contains("LOWER(e) LIKE"));
    }

    #[test]
    fn ix_label_json_mode_builds_set_equality() {
        let b = build_where_and_order(
            &query(serde_json::json!({"f_ix_label": "[\"buy\",\"sell\"]"})),
            now(),
        );
        assert!(b.where_sql.contains("WITH ORDINALITY"));
        match b.args.last() {
            Some(SqlArg::StrArray(a)) => assert_eq!(a, &vec!["buy".to_string(), "sell".to_string()]),
            other => panic!("expected StrArray, got {other:?}"),
        }
    }

    #[test]
    fn global_search_is_mint_and_symbol_only() {
        let b = build_where_and_order(&query(serde_json::json!({"search": "abc"})), now());
        // One bound needle, reused across exactly the mint + symbol columns.
        assert_eq!(b.args.len(), 1);
        assert!(b.where_sql.contains("LOWER(t.mint_address) LIKE"));
        assert!(b.where_sql.contains("LOWER(t.symbol) LIKE"));
        // Narrowed: no more date / numeric / creator / name substring matching.
        assert!(!b.where_sql.contains("to_char("));
        assert!(!b.where_sql.contains("i.trade_count, 0)::text"));
        assert!(!b.where_sql.contains("t.name"));
        assert!(!b.where_sql.contains("creator_wallet"));
    }

    #[test]
    fn lamports_columns_filter_in_sol() {
        let b = build_where_and_order(&query(serde_json::json!({"f_max_cost_lamports_min": "0.5"})), now());
        // buy-ix JSON arg, lamports → SOL via /1e9
        assert!(b.where_sql.contains("/1e9"));
        assert!(b.where_sql.contains("initial_buy_instruction->>'max_cost_lamports'"));
    }
}
