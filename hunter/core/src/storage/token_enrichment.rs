//! Single source of truth for **token enrichment** — the ~28 `tokens` +
//! `tokens_info` metadata fields every token-result table renders (Matched,
//! Positions, Simulated, Sweep drill-in). The JSON flatten struct [`TokenEnrichment`]
//! mirrors the frontend `TOKEN_ENRICH_FIELDS` (`sharedTokenColumns.tsx`) 1:1; the
//! `FromRow` shape [`TokenEnrichmentRow`] is a **superset** — it additionally carries
//! the row-owned `symbol` / `created_at` (aliased `token_created_at`) / `ath_price`
//! that are excluded from the flatten (see the exclusion note below).
//!
//! Three artifacts kept in lockstep so no table re-derives the projection:
//!   - [`ENRICH_SELECT`] — the SQL column fragment (aliases `t` = tokens,
//!     `i` = tokens_info). SQL-paged tables splice it into their `SELECT` and
//!     `#[sqlx(flatten)]` [`TokenEnrichmentRow`] onto their row.
//!   - [`TokenEnrichmentRow`] — the `FromRow` shape. Also the return type of the
//!     batch fetch ([`fetch_by_mints`]) that in-memory tables (Simulated, Sweep)
//!     use to enrich rows they built in Rust.
//!   - [`TokenEnrichment`] — the JSON flatten struct (`#[serde(flatten)]` onto a
//!     result row). `From<&TokenEnrichmentRow>`.
//!
//! **Row-owned fields excluded from [`TokenEnrichment`]:** `mint`, `symbol` (pure
//! identity — every result row already carries them) and `ath_price`, `created_at`.
//! `created_at` diverges per table (Positions' is the position's, not the token's).
//! `ath_price` is uniform `tokens_info.ath_price` everywhere but stays row-owned so a
//! host struct that already declares its own `ath_price` field (Simulated / Positions
//! / Sweep) doesn't collide with a flattened one under `#[serde(flatten)]` — every
//! host sets it off [`TokenEnrichmentRow`] (which carries it), and none recompute it.
//! Token creation is aliased `token_created_at` in SQL so it never collides with
//! `strategy_positions.created_at` under flatten.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;

/// Canonical **market-cap** SQL expression — current spot price × on-chain supply.
/// Requires aliases `t` = tokens, `i` = tokens_info. THE single definition; every
/// SQL site that projects/sorts/filters `market_cap` (this module's [`ENRICH_SELECT`]
/// + sort/filter arms, `handlers::tokens`, `handlers::tokens::sql`, `token_repo`)
/// references this const so the formula can never drift between the token-list SQL
/// backends. The parenthesis is part of the expression so it composes inside
/// `COALESCE(...)`/`(...)::text` without extra wrapping.
pub const MARKET_CAP_SQL: &str = "(i.current_price * t.initial_supply_token)";

/// SQL column fragment for the token-enrichment projection. Requires the query to
/// alias `tokens` as `t` and `tokens_info` as `i` (LEFT JOIN). Column names/order
/// match [`TokenEnrichmentRow`]. `market_cap` is computed ([`MARKET_CAP_SQL`],
/// `current_price × initial_supply_token`) — the canonical definition shared with
/// the token list; a guard test pins this literal to that const.
/// `token_created_at` is deliberately not `created_at` (flatten-collision guard).
pub const ENRICH_SELECT: &str = "t.mint_address, t.symbol, t.name, \
    t.created_at AS token_created_at, t.creator_wallet, t.creation_tx_signature, \
    t.initial_supply_token, t.initial_buy_lamports::float8 / 1e9 AS initial_buy_sol, \
    t.initial_buy_instruction, t.cu_limit, t.cu_price, t.is_mayhem_mode, \
    t.is_cashback_enabled, t.ix_labels, \
    i.ath_price, i.ath_timestamp, i.volume_sol, \
    (i.current_price * t.initial_supply_token) AS market_cap, i.trade_count, \
    i.last_trade_at, i.current_price, i.is_dead, i.is_migrated, \
    (SELECT MAX(s.last_synced_at) FROM token_sync_state s \
       WHERE s.mint_address = t.mint_address) AS last_synced_at, \
    i.first_slot_buy_lamports::float8 / 1e9 AS first_slot_buy_sol, \
    i.first_slot_sell_lamports::float8 / 1e9 AS first_slot_sell_sol";

/// `FromRow` shape for [`ENRICH_SELECT`]. Carries the row-owned readables
/// (`mint_address`, `symbol`, `name`, `token_created_at`, `ath_price`) too, so a
/// host that owns those fields can pull them off here. Flatten-safe against
/// `StrategyPositionDbRow` (no shared column name — `mint_address` ≠ `mint`,
/// `token_created_at` ≠ `created_at`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TokenEnrichmentRow {
    pub mint_address: String,
    pub symbol: String,
    pub name: String,
    pub token_created_at: DateTime<Utc>,
    pub creator_wallet: String,
    pub creation_tx_signature: String,
    pub initial_supply_token: Option<i64>,
    pub initial_buy_sol: Option<f64>,
    pub initial_buy_instruction: Option<Json<Value>>,
    pub cu_limit: Option<i64>,
    pub cu_price: Option<i64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub ix_labels: Json<Value>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub volume_sol: Option<f64>,
    pub market_cap: Option<f64>,
    pub trade_count: Option<i64>,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_dead: Option<bool>,
    pub is_migrated: Option<bool>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub first_slot_buy_sol: Option<f64>,
    pub first_slot_sell_sol: Option<f64>,
}

/// Read a `u64` buy-instruction arg by its snake_case name from
/// `initial_buy_instruction`. (Duplicated from `handlers::tokens` — private there.)
fn buy_arg_u64(value: &Option<Value>, field: &str) -> Option<u64> {
    value.as_ref()?.get(field).and_then(Value::as_u64)
}

/// JSON flatten struct — the ~24 enrichment fields shared by every token-result
/// table's response body. JSON names match `TOKEN_ENRICH_FIELDS` exactly. Excludes
/// `mint`/`symbol`/`ath_price`/`created_at` (row-owned; see module docs).
#[derive(Debug, Clone, Default, Serialize)]
pub struct TokenEnrichment {
    pub name: String,
    pub creator_wallet: String,
    pub creation_tx_signature: String,
    pub initial_buy_sol: Option<f64>,
    pub initial_supply_token: Option<u64>,
    pub token_amount: Option<u64>,
    pub max_cost_lamports: Option<u64>,
    pub spendable_lamports_in: Option<u64>,
    pub min_tokens_out: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub instruction_labels: Value,
    pub ix_labels_count: usize,
    pub current_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub market_cap: Option<f64>,
    pub volume_sol_total: f64,
    pub trade_count: u64,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub first_slot_buy_sol: Option<f64>,
    pub first_slot_sell_sol: Option<f64>,
    pub is_migrated: bool,
    pub is_dead: bool,
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl From<&TokenEnrichmentRow> for TokenEnrichment {
    fn from(r: &TokenEnrichmentRow) -> Self {
        let ix_labels_count = match &r.ix_labels.0 {
            Value::Array(arr) => arr.len(),
            _ => 0,
        };
        let buy_ix = r.initial_buy_instruction.as_ref().map(|j| j.0.clone());
        Self {
            name: r.name.clone(),
            creator_wallet: r.creator_wallet.clone(),
            creation_tx_signature: r.creation_tx_signature.clone(),
            initial_buy_sol: r.initial_buy_sol,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            token_amount: buy_arg_u64(&buy_ix, "token_amount"),
            max_cost_lamports: buy_arg_u64(&buy_ix, "max_cost_lamports"),
            spendable_lamports_in: buy_arg_u64(&buy_ix, "spendable_lamports_in"),
            min_tokens_out: buy_arg_u64(&buy_ix, "min_tokens_out"),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            is_mayhem_mode: r.is_mayhem_mode,
            is_cashback_enabled: r.is_cashback_enabled,
            instruction_labels: r.ix_labels.0.clone(),
            ix_labels_count,
            current_price: r.current_price,
            ath_timestamp: r.ath_timestamp,
            market_cap: r.market_cap,
            volume_sol_total: r.volume_sol.unwrap_or(0.0),
            trade_count: r.trade_count.unwrap_or(0) as u64,
            last_trade_at: r.last_trade_at,
            first_slot_buy_sol: r.first_slot_buy_sol,
            first_slot_sell_sol: r.first_slot_sell_sol,
            is_migrated: r.is_migrated.unwrap_or(false),
            is_dead: r.is_dead.unwrap_or(false),
            last_synced_at: r.last_synced_at,
        }
    }
}

/// Batch-fetch enrichment for `mints` (bounded — `WHERE mint_address = ANY($1)`,
/// never a table scan), for the in-memory tables (Simulated, Sweep) that build rows
/// in Rust. Empty input short-circuits. Results are in unspecified DB order — the
/// caller keys by `mint_address`.
pub async fn fetch_by_mints(
    pool: &PgPool,
    mints: &[String],
) -> anyhow::Result<Vec<TokenEnrichmentRow>> {
    if mints.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {ENRICH_SELECT} FROM tokens t \
         LEFT JOIN tokens_info i ON i.mint_address = t.mint_address \
         WHERE t.mint_address = ANY($1)"
    );
    let rows = sqlx::query_as::<_, TokenEnrichmentRow>(&sql)
        .bind(mints)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Sort/filter whitelist — the token-column (`t.`/`i.`) half of the sort/filter
// mapping. The SSOT for *sortable/filterable* enrichment columns, mirroring the
// `ENRICH_SELECT` projection. SQL-paged hosts layer their own row-owned arms on
// top and fall through here: `strategy_repo::position_*_sql` keeps only its
// `sp.*` arms, `token_*_sql` keeps only its `mint → t.mint_address` alias.
// ---------------------------------------------------------------------------

/// Filter-column type: decides which filter operators are legal and how the
/// value binds. `Text` cols honor substring / exact string match (`ILIKE`);
/// `Numeric` cols honor the comparison ops with the value bound as `f64` (the
/// expr is the **uncast** numeric so the compare is numeric, not `::text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Text,
    Numeric,
}

/// Map a frontend column key to its **trusted** whitelisted `ORDER BY` expression
/// for the enrichment (`t.`/`i.`) columns. `None` = not a known enrichment sort
/// key (the host wrapper decides whether it owns the key). Column set matches
/// [`ENRICH_SELECT`]; both frontend key aliases are accepted where the Matched
/// and Positions tables historically disagreed (`created`/`created_at`,
/// `initial_buy`/`init_buy`).
pub fn enrich_sort_sql(key: &str) -> Option<&'static str> {
    Some(match key {
        // tokens
        "symbol" => "t.symbol",
        "name" => "t.name",
        "creator" => "t.creator_wallet",
        "created" | "created_at" => "t.created_at",
        "initial_buy" | "init_buy" => "t.initial_buy_lamports",
        "init_supply" => "t.initial_supply_token",
        "cu_limit" => "t.cu_limit",
        "cu_price" => "t.cu_price",
        "mayhem_mode" => "t.is_mayhem_mode",
        "cashback" => "t.is_cashback_enabled",
        // tokens_info
        "current_price" => "i.current_price",
        "ath_price" => "i.ath_price",
        "ath_timestamp" => "i.ath_timestamp",
        "market_cap" => MARKET_CAP_SQL,
        "volume" => "i.volume_sol",
        "trade_count" => "i.trade_count",
        "last_trade" => "i.last_trade_at",
        "migrated" => "i.is_migrated",
        "dead" => "i.is_dead",
        "first_slot_buy" => "i.first_slot_buy_lamports",
        "first_slot_sell" => "i.first_slot_sell_lamports",
        // initial_buy_instruction JSONB
        "token_amount" => "(t.initial_buy_instruction->>'token_amount')::numeric",
        "max_cost_lamports" => "(t.initial_buy_instruction->>'max_cost_lamports')::numeric",
        "spendable_lamports_in" => "(t.initial_buy_instruction->>'spendable_lamports_in')::numeric",
        "min_tokens_out" => "(t.initial_buy_instruction->>'min_tokens_out')::numeric",
        _ => return None,
    })
}

/// Map a frontend column key to its **trusted** whitelisted filter expression +
/// type for the enrichment (`t.`/`i.`) columns. `None` = not a known enrichment
/// filter key. `Numeric` returns the **uncast** expr so numeric operators compare
/// numerically. Mirrors [`enrich_sort_sql`]'s columns (the boolean columns aren't
/// filterable, matching the historical whitelist).
pub fn enrich_filter_sql(key: &str) -> Option<(&'static str, FilterKind)> {
    use FilterKind::{Numeric, Text};
    Some(match key {
        // tokens
        "symbol" => ("t.symbol", Text),
        "name" => ("t.name", Text),
        "creator" => ("t.creator_wallet", Text),
        "initial_buy" | "init_buy" => ("t.initial_buy_lamports", Numeric),
        "init_supply" => ("t.initial_supply_token", Numeric),
        "cu_limit" => ("t.cu_limit", Numeric),
        "cu_price" => ("t.cu_price", Numeric),
        // tokens_info
        "current_price" => ("i.current_price", Numeric),
        "ath_price" => ("i.ath_price", Numeric),
        "market_cap" => (MARKET_CAP_SQL, Numeric),
        "volume" => ("i.volume_sol", Numeric),
        "trade_count" => ("i.trade_count", Numeric),
        "first_slot_buy" => ("i.first_slot_buy_lamports", Numeric),
        "first_slot_sell" => ("i.first_slot_sell_lamports", Numeric),
        // initial_buy_instruction JSONB — text in JSONB, cast to numeric so the
        // comparison ops work.
        "token_amount" => ("(t.initial_buy_instruction->>'token_amount')::numeric", Numeric),
        "max_cost_lamports" => ("(t.initial_buy_instruction->>'max_cost_lamports')::numeric", Numeric),
        "spendable_lamports_in" => ("(t.initial_buy_instruction->>'spendable_lamports_in')::numeric", Numeric),
        "min_tokens_out" => ("(t.initial_buy_instruction->>'min_tokens_out')::numeric", Numeric),
        _ => return None,
    })
}

#[cfg(test)]
mod market_cap_ssot_tests {
    use super::*;

    /// The `ENRICH_SELECT` projection embeds the market-cap formula as a literal
    /// (Rust `const` can't concat another `&str`), so pin it to [`MARKET_CAP_SQL`]
    /// here — this fails the moment the projection drifts from the canonical formula.
    #[test]
    fn enrich_select_uses_canonical_market_cap() {
        assert!(
            ENRICH_SELECT.contains(MARKET_CAP_SQL),
            "ENRICH_SELECT must project market_cap via MARKET_CAP_SQL"
        );
    }

    #[test]
    fn market_cap_sort_and_filter_use_canonical() {
        assert_eq!(enrich_sort_sql("market_cap"), Some(MARKET_CAP_SQL));
        assert_eq!(enrich_filter_sql("market_cap"), Some((MARKET_CAP_SQL, FilterKind::Numeric)));
    }
}
