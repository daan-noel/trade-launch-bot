use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::{
    analyzers::ChainStats,
    api::table_query::{FilterOp, FilterSpec, TableRequest},
    state::{core_state::CoreState, token_cache::TokenState},
    storage::token_enrichment::MARKET_CAP_SQL,
};

/// Read a `u64` buy-instruction arg by its snake_case name from
/// `initial_buy_instruction`.
fn extract_buy_arg_u64(value: &Option<Value>, field: &str) -> Option<u64> {
    value.as_ref()?.get(field).and_then(|v| v.as_u64())
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Compact token summary emitted in list responses. `Clone` so the shared list
/// snapshot can materialise just the requested page (see `TokenListCache`).
#[derive(Serialize, Clone)]
pub struct TokenSummary {
    pub mint_address: String,
    pub symbol: String,
    pub current_price: Option<f64>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub volume_sol_total: f64,
    /// Buy/sell SOL summed over trades landing in the token's creation slot
    /// (human SOL; see `TokenState::first_slot_buy_sol`). `None` when the token
    /// predates the metric or has no creation-slot activity recorded.
    pub first_slot_buy_sol: Option<f64>,
    pub first_slot_sell_sol: Option<f64>,
    pub market_cap: Option<f64>,
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
    pub ix_labels_count: usize,
    pub instruction_labels: Value,
    pub is_migrated: bool,
    /// Dead-token verdict (liquidity gone + price back at launch + only dust
    /// trading; see `TokenState::is_dead`). Serialized — unlike `age_seconds` it is
    /// a near-stable boolean (a dead token stays dead), so it only churns the ETag
    /// when deadness actually flips, which is exactly when the UI should refresh.
    pub is_dead: bool,
    /// Computed at snapshot-build time, but NOT serialized: the frontend derives
    /// age from `created_at` so it ticks live, and — more importantly — keeping a
    /// `now`-derived value out of the response body stops it churning the bytes
    /// every poll, which would otherwise defeat the list endpoint's ETag (a
    /// content hash). Still used internally for the `token_age` sort/filter.
    #[serde(skip)]
    pub age_seconds: i64,
    pub created_at: DateTime<Utc>,
    pub creator_address: String,
    pub creation_tx_signature: String,
    pub name: String,
    pub trade_count: u64,
    pub last_trade_at: Option<DateTime<Utc>>,
    /// Seconds from creation to the last meaningful trade; `Some` only when dead.
    pub lifetime_secs: Option<i64>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl From<&TokenState> for TokenSummary {
    fn from(s: &TokenState) -> Self {
        let now = chrono::Utc::now();
        let age_seconds = now.signed_duration_since(s.token.created_at).num_seconds();
        let is_dead = s.is_dead(now);
        let lifetime_secs = s.lifetime_secs(now);

        let ix_labels_count = match &s.token.instruction_labels {
            serde_json::Value::Array(arr) => arr.len(),
            _ => 0,
        };

        Self {
            mint_address: s.token.mint_address.clone(),
            symbol: s.token.symbol.clone(),
            current_price: s.current_price,
            ath_price: s.ath_price,
            ath_timestamp: s.ath_timestamp,
            volume_sol_total: s.volume_sol_total,
            first_slot_buy_sol: Some(s.first_slot_buy_sol),
            first_slot_sell_sol: Some(s.first_slot_sell_sol),
            market_cap: s.market_cap,
            initial_buy_sol: s.token.initial_buy_sol,
            initial_supply_token: s.token.initial_supply_token,
            token_amount: extract_buy_arg_u64(&s.token.initial_buy_instruction, "token_amount"),
            max_cost_lamports: extract_buy_arg_u64(&s.token.initial_buy_instruction, "max_cost_lamports"),
            spendable_lamports_in: extract_buy_arg_u64(
                &s.token.initial_buy_instruction,
                "spendable_lamports_in",
            ),
            min_tokens_out: extract_buy_arg_u64(&s.token.initial_buy_instruction, "min_tokens_out"),
            cu_limit: s.token.cu_limit,
            cu_price: s.token.cu_price,
            is_mayhem_mode: s.token.is_mayhem_mode,
            is_cashback_enabled: s.token.is_cashback_enabled,
            ix_labels_count,
            instruction_labels: s.token.instruction_labels.clone(),
            is_migrated: s.is_migrated,
            is_dead,
            age_seconds,
            created_at: s.token.created_at,
            creator_address: s.token.creator_wallet.clone(),
            creation_tx_signature: s.token.creation_tx_signature.clone(),
            name: s.token.name.clone(),
            trade_count: s.trade_count,
            last_trade_at: s.last_trade_at,
            lifetime_secs,
            last_synced_at: s.last_synced_at,
        }
    }
}

impl From<crate::storage::repositories::token_repo::TokenListRow> for TokenSummary {
    /// Build a list row from a joined `tokens` + `tokens_info` DB row, so the list
    /// can include mints no longer resident in the live cache. `lifetime_secs` is
    /// read directly from the DB column (written at eviction and by the final
    /// metrics flush when a token dies).
    fn from(r: crate::storage::repositories::token_repo::TokenListRow) -> Self {
        let age_seconds = chrono::Utc::now()
            .signed_duration_since(r.created_at)
            .num_seconds();

        let ix_labels_count = match &r.ix_labels.0 {
            serde_json::Value::Array(arr) => arr.len(),
            _ => 0,
        };

        let buy_ix = r.initial_buy_instruction.map(|v| v.0);

        Self {
            mint_address: r.mint_address,
            symbol: r.symbol,
            current_price: r.current_price,
            ath_price: r.ath_price,
            ath_timestamp: r.ath_timestamp,
            volume_sol_total: r.volume_sol.unwrap_or(0.0),
            first_slot_buy_sol: r.first_slot_buy_sol,
            first_slot_sell_sol: r.first_slot_sell_sol,
            market_cap: r.market_cap,
            initial_buy_sol: r.initial_buy_sol,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            token_amount: extract_buy_arg_u64(&buy_ix, "token_amount"),
            max_cost_lamports: extract_buy_arg_u64(&buy_ix, "max_cost_lamports"),
            spendable_lamports_in: extract_buy_arg_u64(&buy_ix, "spendable_lamports_in"),
            min_tokens_out: extract_buy_arg_u64(&buy_ix, "min_tokens_out"),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            is_mayhem_mode: r.is_mayhem_mode,
            is_cashback_enabled: r.is_cashback_enabled,
            ix_labels_count,
            instruction_labels: r.ix_labels.0,
            is_migrated: r.is_migrated.unwrap_or(false),
            is_dead: r.is_dead.unwrap_or(false),
            age_seconds,
            created_at: r.created_at,
            creator_address: r.creator_wallet,
            creation_tx_signature: r.creation_tx_signature,
            name: r.name,
            trade_count: r.trade_count.unwrap_or(0) as u64,
            last_trade_at: r.last_trade_at,
            lifetime_secs: r.lifetime_secs,
            last_synced_at: r.last_synced_at,
        }
    }
}

/// Full token detail including live stats.
#[derive(Serialize)]
pub struct TokenDetail {
    pub mint_address: String,
    pub name: String,
    pub symbol: String,
    pub creator_address: String,
    pub bonding_curve_address: Option<String>,
    pub initial_supply_token: Option<u64>,
    pub initial_buy_sol: Option<f64>,
    pub token_amount: Option<u64>,
    pub max_cost_lamports: Option<u64>,
    pub spendable_lamports_in: Option<u64>,
    pub min_tokens_out: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub instruction_labels: serde_json::Value,
    pub creation_tx_signature: String,
    pub created_at: DateTime<Utc>,
    // Non-null (coalesced to 0), matching `TokenSummary`/the list endpoint — the
    // detail modal and the list agree on these two counters' shape (SSOT reconcile).
    pub trade_count: u64,
    pub volume_sol_total: f64,
    pub first_slot_buy_sol: Option<f64>,
    pub first_slot_sell_sol: Option<f64>,
    pub market_cap: Option<f64>,
    pub current_price: Option<f64>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub is_migrated: bool,
    pub unique_wallets: Option<usize>,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl From<&TokenState> for TokenDetail {
    fn from(s: &TokenState) -> Self {
        Self {
            mint_address: s.token.mint_address.clone(),
            name: s.token.name.clone(),
            symbol: s.token.symbol.clone(),
            creator_address: s.token.creator_wallet.clone(),
            bonding_curve_address: s.token.bonding_curve_address.clone(),
            initial_supply_token: s.token.initial_supply_token,
            initial_buy_sol: s.token.initial_buy_sol,
            token_amount: extract_buy_arg_u64(&s.token.initial_buy_instruction, "token_amount"),
            max_cost_lamports: extract_buy_arg_u64(&s.token.initial_buy_instruction, "max_cost_lamports"),
            spendable_lamports_in: extract_buy_arg_u64(
                &s.token.initial_buy_instruction,
                "spendable_lamports_in",
            ),
            min_tokens_out: extract_buy_arg_u64(&s.token.initial_buy_instruction, "min_tokens_out"),
            cu_limit: s.token.cu_limit,
            cu_price: s.token.cu_price,
            is_mayhem_mode: s.token.is_mayhem_mode,
            is_cashback_enabled: s.token.is_cashback_enabled,
            instruction_labels: s.token.instruction_labels.clone(),
            creation_tx_signature: s.token.creation_tx_signature.clone(),
            created_at: s.token.created_at,
            trade_count: s.trade_count,
            volume_sol_total: s.volume_sol_total,
            first_slot_buy_sol: Some(s.first_slot_buy_sol),
            first_slot_sell_sol: Some(s.first_slot_sell_sol),
            market_cap: s.market_cap,
            current_price: s.current_price,
            ath_price: s.ath_price,
            ath_timestamp: s.ath_timestamp,
            is_migrated: s.is_migrated,
            unique_wallets: Some(s.unique_wallets()),
            last_trade_at: s.last_trade_at,
            last_synced_at: s.last_synced_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TokensListResponse {
    /// Filtered count over the whole merged universe (live cache overlaying the
    /// DB base) — what the table's pager needs.
    pub total: usize,
    /// Filtered count restricted to the live, cache-tracked subset. Always
    /// `<= total`; the UI shows it alongside `total` as "tracked vs all".
    pub tracked: usize,
    pub items: Vec<TokenSummary>,
}

// The token list is requested via the unified `POST /api/tokens` body
// [`trading_core::api::table_query::TableRequest`] — the SAME contract the strategy
// tables use. The global `TokenFilters` panel and the DataTable per-column filters
// both arrive as entries in its `filters: {col → FilterSpec}` map; the Tokens-only
// `tracked_only` / `swing_run_id` / `swing_chain_latency_ms` fields ride alongside.
// [`TokenQuery::from_table_request`] lowers that body onto this module's internal
// representation (`f` panel map + `col_filters`), which the two eval engines
// (`matches` in-RAM, `sql.rs` SQL) already consume unchanged.

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Filter → sort → page → serialize+ETag the token list (the CPU core of
/// `list_tokens`). The swing-sort stats map is passed in rather than computed
/// here, so this stays free of any swing/analysis dependency: a deploy build
/// calls it with `swing_stats = None`; a local build computes the map first.
/// Returns the response body bytes and their content-hash ETag.
pub fn build_tokens_list(
    state: &CoreState,
    q: &TokenQuery,
    limit_q: i64,
    offset_q: i64,
    tracked_only: bool,
    swing_stats: Option<&HashMap<String, ChainStats>>,
) -> (Vec<u8>, String) {
    let now = chrono::Utc::now();

    // Shared, pre-sorted (newest-first) snapshot of the whole list. Rebuilt at
    // most once per staleness window across all clients, so a request no longer
    // clones the entire cache on every poll.
    let snapshot = state.token_list.get(&state.token_cache, now);

    // When `tracked_only`, restrict to the live cache subset; otherwise use the
    // full merged universe (live cache overlaying the DB base).
    let mut matched: Vec<&TokenSummary> = if tracked_only {
        snapshot.tracked_filtered(|t| q.matches(t, now))
    } else {
        // Full-fidelity server-side reduction: global filters + search + per-column
        // filters (mirrors `tokenPassesFilters` and the DataTable). Filter by
        // reference — non-matching rows are never cloned. The snapshot merges the
        // live cache over the DB base (whole seeded universe), so the list includes
        // mints already evicted from the cache, newest-first.
        snapshot.merged_filtered(|t| q.matches(t, now))
    };

    // `total` is the FILTERED count — that's what the table's pager needs.
    let total = matched.len();

    // Same reduction, restricted to the live cache-tracked subset. Cheap: the
    // resident set is small (post-eviction) relative to the merged universe.
    // When already in tracked_only mode, `total` == `tracked`.
    let tracked = if tracked_only {
        total
    } else {
        snapshot.tracked_filtered_count(|t| q.matches(t, now))
    };

    // The snapshot is already newest-first, so the default view needs no sort;
    // only explicit sort levels re-order (sorting precedes paging). Non-matched
    // mints absent from `swing_stats` simply sort last.
    q.sort_refs(&mut matched, swing_stats);

    let limit = limit_q.max(1).min(50_000) as usize;
    let offset = offset_q.max(0) as usize;
    // Materialise (clone) only the requested page.
    let items: Vec<TokenSummary> = matched
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    let resp = TokensListResponse { total, tracked, items };

    // Serialize + fingerprint here, off the async worker pool. The ETag is a
    // content hash of the page bytes, so a poll that produces a byte-identical
    // page (no new tokens/trades — `age` is no longer in the body, so it no
    // longer churns) can revalidate to a bodyless 304 instead of resending.
    let body = serde_json::to_vec(&resp).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    let etag = format!("\"{:016x}\"", hasher.finish());
    (body, etag)
}

// `list_tokens` (the `GET /api/tokens` handler) lives in the `backend` crate
// (`api::handlers::tokens::list`) because it takes `LocalState` and computes swing
// stats; it calls the core `build_tokens_list` + `is_swing_sort_col` below.

/// `GET /api/tokens/:mint` — token detail from in-memory cache; falls back to DB.
pub async fn get_token(state: web::Data<Arc<CoreState>>, path: web::Path<String>) -> impl Responder {
    let mint = path.into_inner();

    // Fast path: served from cache
    if let Some(entry) = state.token_cache.get(&mint) {
        return HttpResponse::Ok().json(TokenDetail::from(entry.value()));
    }

    // Slow path: token was created before this server session; join tokens_info for stats
    let repo = state.token_repo();
    match repo.find_list_row_by_mint(&mint).await {
        Ok(Some(token)) => {
            let buy_arg = |field: &str| {
                token.initial_buy_instruction.as_ref().and_then(|ix| ix.get(field).and_then(|v| v.as_u64()))
            };
            HttpResponse::Ok().json(serde_json::json!({
                "mint_address": token.mint_address,
                "name": token.name,
                "symbol": token.symbol,
                "creator_address": token.creator_wallet,
                "bonding_curve_address": token.bonding_curve_address,
                "initial_supply_token": token.initial_supply_token,
                "initial_buy_sol": token.initial_buy_sol,
                "token_amount": buy_arg("token_amount"),
                "max_cost_lamports": buy_arg("max_cost_lamports"),
                "spendable_lamports_in": buy_arg("spendable_lamports_in"),
                "min_tokens_out": buy_arg("min_tokens_out"),
                "cu_limit": token.cu_limit,
                "cu_price": token.cu_price,
                "is_mayhem_mode": token.is_mayhem_mode,
                "is_cashback_enabled": token.is_cashback_enabled,
                "instruction_labels": token.ix_labels,
                "creation_tx_signature": token.creation_tx_signature,
                "created_at": token.created_at,
                "trade_count": token.trade_count,
                "volume_sol_total": token.volume_sol,
                "first_slot_buy_sol": token.first_slot_buy_sol,
                "first_slot_sell_sol": token.first_slot_sell_sol,
                "market_cap": token.market_cap,
                "current_price": token.current_price,
                "ath_price": token.ath_price,
                "ath_timestamp": token.ath_timestamp,
                "is_migrated": token.is_migrated.unwrap_or(false),
                "unique_wallets": null,
                "last_trade_at": token.last_trade_at,
                "last_synced_at": token.last_synced_at,
            }))
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "token not found",
            "mint_address": mint
        })),
        Err(e) => {
            tracing::error!("DB error fetching token {mint}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }))
        }
    }
}

/// Query params for `get_trades`. Bounds the response so a high-volume token
/// can't return an unbounded list (which would block a request thread on the
/// clone/serialize). Chronological order is preserved; page with `offset`.
#[derive(Deserialize)]
pub struct TradesPageParams {
    #[serde(default = "default_trades_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_trades_limit() -> i64 {
    5_000
}

/// Hard cap on `offset` for the DB-fallback path. `find_by_mint_paged` uses
/// `LIMIT/OFFSET`, so a large offset makes Postgres scan-and-discard `offset`
/// rows of a high-volume `trades` partition on every request. The cache branch
/// (the common case) pages an in-memory `Vec` and is unaffected; real clients
/// only ever request `offset = 0`, so this just bounds abusive deep paging.
/// Deep history should move to keyset/seek paging if it's ever needed.
const MAX_TRADES_OFFSET: i64 = 50_000;

/// `GET /api/tokens/:mint/trades`
///
/// Returns full `Trade` rows for a token in chronological order from the DB,
/// bounded by `limit` (default & cap 5000) and `offset`. Reads from Postgres
/// rather than the live cache: the `TokenCache` now retains only a slimmed
/// `CachedTrade` projection (missing the `id`/`instruction_labels`/… fields this
/// endpoint serializes), and this is a cold, paginated path off the hot loop.
pub async fn get_trades(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    query: web::Query<TradesPageParams>,
) -> impl Responder {
    let mint = path.into_inner();
    let limit = query.limit.clamp(1, 5_000);
    let offset = query.offset.clamp(0, MAX_TRADES_OFFSET);

    let repo = state.trade_repo();
    match repo.find_by_mint_paged(&mint, limit, offset).await {
        Ok(trades) => HttpResponse::Ok().json(trades),
        Err(e) => {
            tracing::error!("DB error fetching trades for {mint}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }))
        }
    }
}

// ===========================================================================
// Server-side filtering / sorting engine
//
// Faithful Rust port of the React Tokens table's client-side reduction so the
// whole token set can be filtered/sorted/paged on the server. Mirrors:
//   - tokenPassesFilters (frontend-react/src/components/tokens/filters.ts)
//   - the DataTable global search + per-column filters (DataTable.tsx)
//   - parseNumericPredicate grammar (numericFilter.ts)
//   - compareSort null/dir semantics (DataTable.tsx)
//
// Intentional deviation: formatted-value substring matching (compact "1.2K",
// price strings) is not reproduced — numeric columns match via the numeric
// grammar, otherwise we fall back to the raw stringified value.
// ===========================================================================

/// Inactivity window after which a token's lifetime is treated as final for the
/// short-lived filter (mirrors filters.ts `LIFETIME_STALE_MS`). Distinct from the
/// `is_dead` flag, which is a richer liquidity/price/volume verdict.
const LIFETIME_STALE_MS: i64 = 60 * 60 * 1000;

// ===========================================================================
// Column-grammar registry — the SINGLE source of truth for the token-list
// filter/sort grammar, read by BOTH engines:
//   * the SQL backend (`sql.rs`, `live`): `sql_num` / `sql_text` / `sql_sort`
//   * the in-RAM evaluator (`lab` + Simulated): `ram_num` / `ram_text` / `ram_sort`
// A column is declared exactly ONCE here, so its SQL expression and its
// `TokenSummary` accessor can never silently drift (previously two parallel match
// families held at parity only by tests). `sql_num`/`ram_num` are `Some` iff the
// column takes the numeric per-column-filter grammar (the old `NUMERIC_COLS`);
// `sql_sort`/`ram_sort` are `Some` iff it is sortable (the old `SORTABLE_COLS`).
// Browser-derived swing-chain columns have no `TokenSummary` field and stay
// outside the registry (see `is_swing_sort_col`).
// ===========================================================================

type RamText = fn(&TokenSummary) -> String;
type RamNum = fn(&TokenSummary) -> Option<f64>;
type RamSort = fn(&TokenSummary) -> SortKey;

/// One column's complete filter/sort grammar for both engines.
struct ColumnSpec {
    key: &'static str,
    /// SQL text projection for a substring filter; `None` ⇒ not text-filterable in
    /// SQL (only `ix_labels`, whose labels live in JSONB — in-RAM-only, as before).
    sql_text: Option<String>,
    ram_text: RamText,
    /// Numeric filter expression (nullable numeric). `Some` ⇒ numeric-filterable.
    sql_num: Option<String>,
    ram_num: Option<RamNum>,
    /// Sort expression + whether it's a text sort (caller wraps in `LOWER`).
    /// `Some` ⇒ sortable.
    sql_sort: Option<(String, bool)>,
    ram_sort: Option<RamSort>,
}

impl ColumnSpec {
    fn new(key: &'static str, sql_text: Option<String>, ram_text: RamText) -> Self {
        Self { key, sql_text, ram_text, sql_num: None, ram_num: None, sql_sort: None, ram_sort: None }
    }
    /// Declare the column sortable: SQL sort expr (`is_text` ⇒ case-insensitive at
    /// the caller) paired with its in-RAM sort key.
    fn sortable(mut self, expr: impl Into<String>, is_text: bool, ram: RamSort) -> Self {
        self.sql_sort = Some((expr.into(), is_text));
        self.ram_sort = Some(ram);
        self
    }
    /// Declare the column numeric-filterable: SQL numeric expr paired with its
    /// in-RAM numeric accessor.
    fn numeric(mut self, expr: impl Into<String>, ram: RamNum) -> Self {
        self.sql_num = Some(expr.into());
        self.ram_num = Some(ram);
        self
    }
}

/// Build the column registry. Owned `String` SQL exprs (several are computed) +
/// zero-cost `fn`-pointer `TokenSummary` accessors. Constructed once (see
/// [`registry`]). Sort expressions that need a JSON/CASE body reuse the `*_SORT`
/// constants; filter exprs reuse the `*_sql`/`*_sql_expr` helpers, so the SQL and
/// in-RAM sides of each column are literally the same declaration.
fn build_registry() -> Vec<ColumnSpec> {
    use ColumnSpec as C;
    vec![
        // --- identity / text ---
        C::new("symbol", Some("(t.symbol || ' ' || t.name || ' ' || t.mint_address)".into()),
                |t| format!("{} {} {}", t.symbol, t.name, t.mint_address))
            .sortable("t.symbol", true, |t| SortKey::Str(Some(t.symbol.clone()))),
        C::new("name", Some("t.name".into()), |t| t.name.clone())
            .sortable("t.name", true, |t| SortKey::Str(Some(t.name.clone()))),
        C::new("mint_address", Some("t.mint_address".into()), |t| t.mint_address.clone())
            .sortable("t.mint_address", true, |t| SortKey::Str(Some(t.mint_address.clone()))),
        C::new("creator", Some("t.creator_wallet".into()), |t| t.creator_address.clone())
            .sortable("t.creator_wallet", true, |t| SortKey::Str(Some(t.creator_address.clone()))),
        C::new("create_tx", Some("t.creation_tx_signature".into()), |t| t.creation_tx_signature.clone()),
        C::new("token_age", Some("EXTRACT(EPOCH FROM (now() - t.created_at))::bigint::text".into()),
                |t| t.age_seconds.to_string())
            .sortable("EXTRACT(EPOCH FROM (now() - t.created_at))", false,
                |t| SortKey::Num(Some(t.age_seconds as f64))),
        // Timestamps sort/filter as their RFC3339 string (lexical == chronological).
        C::new("created", Some(rfc3339_sql("t.created_at")), |t| t.created_at.to_rfc3339())
            .sortable("t.created_at", false, |t| SortKey::Str(Some(t.created_at.to_rfc3339()))),
        C::new("last_trade", Some(rfc3339_sql("i.last_trade_at")),
                |t| opt_num_str(t.last_trade_at.map(|d| d.to_rfc3339())))
            .sortable("i.last_trade_at", false, |t| SortKey::Str(t.last_trade_at.map(|d| d.to_rfc3339()))),
        C::new("lifetime", Some("COALESCE(i.lifetime_secs::text, '')".into()), |t| opt_num_str(t.lifetime_secs))
            .sortable("i.lifetime_secs", false, |t| SortKey::Num(t.lifetime_secs.map(|v| v as f64))),
        C::new("last_synced", Some(rfc3339_sql("sync.last_synced_at")),
                |t| opt_num_str(t.last_synced_at.map(|d| d.to_rfc3339())))
            .sortable("sync.last_synced_at", false, |t| SortKey::Str(t.last_synced_at.map(|d| d.to_rfc3339()))),
        C::new("ath_timestamp", Some(rfc3339_sql("i.ath_timestamp")),
                |t| opt_num_str(t.ath_timestamp.map(|d| d.to_rfc3339())))
            .sortable("i.ath_timestamp", false, |t| SortKey::Str(t.ath_timestamp.map(|d| d.to_rfc3339()))),
        // --- numeric (sortable + numeric-filterable) ---
        C::new("trade_count", Some("COALESCE(i.trade_count, 0)::text".into()), |t| t.trade_count.to_string())
            .numeric("COALESCE(i.trade_count, 0)", |t| Some(t.trade_count as f64))
            .sortable("COALESCE(i.trade_count, 0)", false, |t| SortKey::Num(Some(t.trade_count as f64))),
        C::new("volume", Some("COALESCE(i.volume_sol, 0)::text".into()), |t| t.volume_sol_total.to_string())
            .numeric("COALESCE(i.volume_sol, 0)", |t| Some(t.volume_sol_total))
            .sortable("COALESCE(i.volume_sol, 0)", false, |t| SortKey::Num(Some(t.volume_sol_total))),
        C::new("market_cap", Some(format!("COALESCE({MARKET_CAP_SQL}::text, '')")), |t| opt_num_str(t.market_cap))
            .numeric(MARKET_CAP_SQL, |t| t.market_cap)
            .sortable(MARKET_CAP_SQL, false, |t| SortKey::Num(t.market_cap)),
        C::new("ath_fep_ratio", Some(format!("COALESCE(({})::text, '')", ath_fep_sql_expr())),
                |t| opt_num_str(ath_fep_of(t)))
            .numeric(ath_fep_sql_expr(), ath_fep_of)
            .sortable(ATH_FEP_SORT, false, |t| SortKey::Num(ath_fep_of(t))),
        C::new("current_fep_ratio", Some(format!("COALESCE(({})::text, '')", cur_fep_sql_expr())),
                |t| opt_num_str(cur_fep_of(t)))
            .numeric(cur_fep_sql_expr(), cur_fep_of)
            .sortable(CUR_FEP_SORT, false, |t| SortKey::Num(cur_fep_of(t))),
        C::new("first_slot_buy", Some("COALESCE((i.first_slot_buy_lamports::float8/1e9)::text, '')".into()),
                |t| opt_num_str(t.first_slot_buy_sol))
            .numeric("i.first_slot_buy_lamports::float8/1e9", |t| t.first_slot_buy_sol)
            // sort on raw lamports (monotonic with /1e9 — cheaper, no cast).
            .sortable("i.first_slot_buy_lamports", false, |t| SortKey::Num(t.first_slot_buy_sol)),
        C::new("first_slot_sell", Some("COALESCE((i.first_slot_sell_lamports::float8/1e9)::text, '')".into()),
                |t| opt_num_str(t.first_slot_sell_sol))
            .numeric("i.first_slot_sell_lamports::float8/1e9", |t| t.first_slot_sell_sol)
            .sortable("i.first_slot_sell_lamports", false, |t| SortKey::Num(t.first_slot_sell_sol)),
        C::new("initial_buy", Some("COALESCE((t.initial_buy_lamports::float8/1e9)::text, '')".into()),
                |t| opt_num_str(t.initial_buy_sol))
            .numeric("t.initial_buy_lamports::float8/1e9", |t| t.initial_buy_sol)
            .sortable("t.initial_buy_lamports", false, |t| SortKey::Num(t.initial_buy_sol)),
        C::new("init_supply", Some("COALESCE(t.initial_supply_token::text, '')".into()),
                |t| opt_num_str(t.initial_supply_token))
            .numeric("t.initial_supply_token::float8", |t| t.initial_supply_token.map(|v| v as f64))
            .sortable("t.initial_supply_token", false, |t| SortKey::Num(t.initial_supply_token.map(|v| v as f64))),
        C::new("token_amount", Some(format!("COALESCE(({})::text, '')", buy_arg_sql("token_amount"))),
                |t| opt_num_str(t.token_amount))
            .numeric(buy_arg_sql("token_amount"), |t| t.token_amount.map(|v| v as f64))
            .sortable(TOKEN_AMOUNT_SORT, false, |t| SortKey::Num(t.token_amount.map(|v| v as f64))),
        C::new("max_cost_lamports", Some(format!("COALESCE(({}/1e9)::text, '')", buy_arg_sql("max_cost_lamports"))),
                |t| opt_num_str(t.max_cost_lamports.map(|v| v as f64 / 1e9)))
            .numeric(format!("({}/1e9)", buy_arg_sql("max_cost_lamports")),
                |t| t.max_cost_lamports.map(|v| v as f64 / 1e9))
            // sort on raw lamports (monotonic with /1e9).
            .sortable(MAX_SOL_COST_SORT, false, |t| SortKey::Num(t.max_cost_lamports.map(|v| v as f64))),
        C::new("spendable_lamports_in", Some(format!("COALESCE(({}/1e9)::text, '')", buy_arg_sql("spendable_lamports_in"))),
                |t| opt_num_str(t.spendable_lamports_in.map(|v| v as f64 / 1e9)))
            .numeric(format!("({}/1e9)", buy_arg_sql("spendable_lamports_in")),
                |t| t.spendable_lamports_in.map(|v| v as f64 / 1e9))
            .sortable(SPENDABLE_SORT, false, |t| SortKey::Num(t.spendable_lamports_in.map(|v| v as f64))),
        C::new("min_tokens_out", Some(format!("COALESCE(({})::text, '')", buy_arg_sql("min_tokens_out"))),
                |t| opt_num_str(t.min_tokens_out))
            .numeric(buy_arg_sql("min_tokens_out"), |t| t.min_tokens_out.map(|v| v as f64))
            .sortable(MIN_TOKENS_OUT_SORT, false, |t| SortKey::Num(t.min_tokens_out.map(|v| v as f64))),
        C::new("cu_limit", Some("COALESCE(t.cu_limit::text, '')".into()), |t| opt_num_str(t.cu_limit))
            .numeric("t.cu_limit::float8", |t| t.cu_limit.map(|v| v as f64))
            .sortable("t.cu_limit", false, |t| SortKey::Num(t.cu_limit.map(|v| v as f64))),
        C::new("cu_price", Some("COALESCE(t.cu_price::text, '')".into()), |t| opt_num_str(t.cu_price))
            .numeric("t.cu_price::float8", |t| t.cu_price.map(|v| v as f64))
            .sortable("t.cu_price", false, |t| SortKey::Num(t.cu_price.map(|v| v as f64))),
        C::new("ix_count", Some(format!("{}::text", ix_count_sql())), |t| t.ix_labels_count.to_string())
            .numeric(ix_count_sql(), |t| Some(t.ix_labels_count as f64))
            .sortable(IX_COUNT_SORT, false, |t| SortKey::Num(Some(t.ix_labels_count as f64))),
        // --- numeric (nullable): sortable + numeric-filterable. The panel filters
        //     these as opt_f64 ranges; making them `is_numeric_col` lets the unified
        //     per-column path handle both the panel range and a per-column predicate.
        C::new("ath_price", Some("COALESCE(i.ath_price::text, '')".into()), |t| opt_num_str(t.ath_price))
            .numeric("i.ath_price", |t| t.ath_price)
            .sortable("i.ath_price", false, |t| SortKey::Num(t.ath_price)),
        C::new("current_price", Some("COALESCE(i.current_price::text, '')".into()), |t| opt_num_str(t.current_price))
            .numeric("i.current_price", |t| t.current_price)
            .sortable("i.current_price", false, |t| SortKey::Num(t.current_price)),
        // --- flags (sortable as 0/1; substring text filter) ---
        C::new("migrated", Some("COALESCE(i.is_migrated, false)::text".into()), |t| t.is_migrated.to_string())
            .sortable("(COALESCE(i.is_migrated, false))::int", false,
                |t| SortKey::Num(Some(if t.is_migrated { 1.0 } else { 0.0 }))),
        C::new("dead", Some("COALESCE(i.is_dead, false)::text".into()), |t| t.is_dead.to_string())
            .sortable("(COALESCE(i.is_dead, false))::int", false,
                |t| SortKey::Num(Some(if t.is_dead { 1.0 } else { 0.0 }))),
        C::new("mayhem_mode", Some("t.is_mayhem_mode::text".into()), |t| t.is_mayhem_mode.to_string())
            .sortable("t.is_mayhem_mode::int", false,
                |t| SortKey::Num(Some(if t.is_mayhem_mode { 1.0 } else { 0.0 }))),
        C::new("cashback", Some("t.is_cashback_enabled::text".into()), |t| t.is_cashback_enabled.to_string())
            .sortable("t.is_cashback_enabled::int", false,
                |t| SortKey::Num(Some(if t.is_cashback_enabled { 1.0 } else { 0.0 }))),
        // --- in-RAM-only text filter (JSONB labels; no SQL projection, as before) ---
        C::new("ix_labels", None, |t| ix_label_list(&t.instruction_labels).join(", ")),
    ]
}

/// The built column registry, keyed by frontend column key. Built once.
fn registry() -> &'static std::collections::HashMap<&'static str, ColumnSpec> {
    static REG: std::sync::OnceLock<std::collections::HashMap<&'static str, ColumnSpec>> =
        std::sync::OnceLock::new();
    REG.get_or_init(|| build_registry().into_iter().map(|c| (c.key, c)).collect())
}

/// Look up a column spec by frontend key (`None` for unknown / swing-chain keys).
fn column(key: &str) -> Option<&'static ColumnSpec> {
    registry().get(key)
}

/// The chain columns whose sort key comes from a swing run rather than a
/// `TokenSummary` field. Default chain latency when the client omits it.
const SWING_SORT_COLS: &[&str] = &["swing_pairs", "max_seq_pairs", "chain_count"];
const DEFAULT_CHAIN_LATENCY_MS: i64 = 60_000;

pub fn is_swing_sort_col(col: &str) -> bool {
    SWING_SORT_COLS.contains(&col)
}

/// Whether a per-column filter key understands the numeric grammar (registry
/// `sql_num`/`ram_num` present). Exposed for the SQL backend.
pub fn is_numeric_col(key: &str) -> bool {
    column(key).is_some_and(|c| c.sql_num.is_some())
}

/// Whether a sort key is accepted (a registry column with a sort projection, or a
/// browser-derived swing-chain column).
fn is_sortable_key(col: &str) -> bool {
    column(col).is_some_and(|c| c.sql_sort.is_some()) || is_swing_sort_col(col)
}

// ---------------------------------------------------------------------------
// SQL column-expression maps (parity with the in-RAM `TokenSummary` accessors).
//
// These name the `tokens t LEFT JOIN tokens_info i` SQL expression for each
// sortable / filterable column, so the SQL backend in `sql.rs` builds WHERE/ORDER
// from the SAME column semantics the in-RAM engine uses. A change to how a column
// is computed must land in BOTH the `TokenSummary` accessor and here.
// ---------------------------------------------------------------------------

/// FEP entry price = initial_buy_sol(SOL) / initial_supply_token. NULL when either
/// input is missing or supply is 0. ath_fep = ath_price / entry (entry>0).
pub fn ath_fep_sql_expr() -> &'static str {
    "(CASE WHEN t.initial_buy_lamports IS NOT NULL AND t.initial_supply_token > 0 \
            AND (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) > 0 \
            AND i.ath_price IS NOT NULL \
       THEN i.ath_price / (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) END)"
}

/// current_fep = current_price / entry (entry>0).
pub fn cur_fep_sql_expr() -> &'static str {
    "(CASE WHEN t.initial_buy_lamports IS NOT NULL AND t.initial_supply_token > 0 \
            AND (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) > 0 \
            AND i.current_price IS NOT NULL \
       THEN i.current_price / (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) END)"
}

/// SQL numeric expression for a per-column numeric filter key (registry `sql_num`).
/// `None` for unknown / non-numeric keys.
pub fn col_filter_number_sql(key: &str) -> Option<String> {
    column(key).and_then(|c| c.sql_num.clone())
}

/// SQL text expression for a per-column substring filter key (registry `sql_text`).
/// `None` for unknown keys and for `ix_labels` (JSONB, in-RAM-only).
pub fn col_filter_text_sql(key: &str) -> Option<String> {
    column(key).and_then(|c| c.sql_text.clone())
}

/// SQL sort expression for a sortable column, plus whether it's a text sort (so the
/// caller applies a case-insensitive `LOWER`). Registry `sql_sort`. Dir/null
/// semantics are applied by the caller via `NULLS LAST`. `None` for unknown /
/// swing-chain columns.
pub fn sort_sql_expr(col: &str) -> Option<(String, bool)> {
    column(col).and_then(|c| c.sql_sort.clone())
}

// Sort-expression constants that need a JSON/CASE body inline as a `&'static str`.
const ATH_FEP_SORT: &str = "(CASE WHEN t.initial_buy_lamports IS NOT NULL AND t.initial_supply_token > 0 AND (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) > 0 AND i.ath_price IS NOT NULL THEN i.ath_price / (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) END)";
const CUR_FEP_SORT: &str = "(CASE WHEN t.initial_buy_lamports IS NOT NULL AND t.initial_supply_token > 0 AND (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) > 0 AND i.current_price IS NOT NULL THEN i.current_price / (t.initial_buy_lamports::float8/1e9 / t.initial_supply_token) END)";
const TOKEN_AMOUNT_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'token_amount' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'token_amount')::float8 END)";
const MAX_SOL_COST_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'max_cost_lamports' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'max_cost_lamports')::float8 END)";
const SPENDABLE_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'spendable_lamports_in' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'spendable_lamports_in')::float8 END)";
const MIN_TOKENS_OUT_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'min_tokens_out' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'min_tokens_out')::float8 END)";
const IX_COUNT_SORT: &str = "COALESCE(jsonb_array_length(CASE WHEN jsonb_typeof(t.ix_labels) = 'array' THEN t.ix_labels WHEN jsonb_typeof(t.ix_labels->'instructions') = 'array' THEN t.ix_labels->'instructions' ELSE '[]'::jsonb END), 0)";

/// buy-ix JSON numeric reader for the col-filter map.
fn buy_arg_sql(field: &str) -> String {
    format!("(CASE WHEN t.initial_buy_instruction->>'{field}' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'{field}')::float8 END)")
}

fn ix_count_sql() -> &'static str {
    IX_COUNT_SORT
}

/// RFC3339 rendering of a nullable timestamptz as text (matches `to_rfc3339()`),
/// empty string when NULL (matches `opt_num_str(...to_rfc3339())`).
fn rfc3339_sql(col: &str) -> String {
    format!("COALESCE(to_char({col} AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS+00:00'), '')")
}

/// Public wrapper around `parse_dt` for the SQL backend.
pub fn parse_dt_public(v: &str) -> Option<DateTime<Utc>> {
    parse_dt(v)
}

/// Resolve the unified request's ordered sort specs into validated
/// `(column, descending)` levels, index 0 = primary. Unknown columns are dropped
/// (not fatal), so a stale client never breaks the listing.
fn sort_levels_from_specs(sorting: &[crate::api::table_query::SortSpec]) -> Vec<(String, bool)> {
    sorting
        .iter()
        .filter_map(|s| is_sortable_key(&s.col).then(|| (s.col.clone(), s.dir.is_desc())))
        .collect()
}

/// Pick a chain stat as the numeric sort key for a swing column.
fn swing_sort_value(col: &str, s: &crate::analyzers::ChainStats) -> f64 {
    match col {
        "swing_pairs" => s.swing_pairs as f64,
        "max_seq_pairs" => s.max_seq_pairs as f64,
        "chain_count" => s.chain_count as f64,
        _ => 0.0,
    }
}

/// Parsed query: global filters + search + per-column filters + sort.
#[derive(Clone)]
pub struct TokenQuery {
    search: String,
    /// Ordered sort levels `(column, descending)`; index 0 is the primary key.
    /// Empty ⇒ keep the snapshot's default (newest-first) order.
    sort_levels: Vec<(String, bool)>,
    /// (column key, raw expression)
    col_filters: Vec<(String, String)>,
    /// Pasted mint-set filter (the `<MintSetInput>` `in` op on `mint`): exact,
    /// case-sensitive membership over `mint_address`. Empty ⇒ no constraint. Both
    /// eval engines (`matches`, `sql.rs`) honor it identically (parity-guarded).
    mint_in: Vec<String>,
    /// global filter values keyed by TokenFilters field name (non-empty only)
    f: HashMap<&'static str, String>,
    /// Swing run to read chain stats from when sorting a chain column.
    swing_run_id: Option<String>,
    /// Chain-latency budget (ms) used to group those stats.
    swing_chain_latency_ms: i64,
}

/// Insert a non-empty panel-filter value into the `f` map (empty ⇒ inactive, skip).
fn put_str(f: &mut HashMap<&'static str, String>, k: &'static str, v: &str) {
    if !v.is_empty() {
        f.insert(k, v.to_string());
    }
}

/// Look up a global-filter value (defaults to "" — i.e. inactive).
fn g<'a>(f: &'a HashMap<&'static str, String>, k: &str) -> &'a str {
    f.get(k).map(String::as_str).unwrap_or("")
}

// ---------------------------------------------------------------------------
// FilterSpec → internal representation (the inverse of the frontend serializer)
// ---------------------------------------------------------------------------

/// Operand `Value` → trimmed string (numbers stringified, bools as `"true"`/`"false"`);
/// empty for null/other. The frontend sends numeric operands as JSON numbers, and the
/// date/text/flag operands as strings.
fn operand_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        _ => String::new(),
    }
}

/// A numeric `FilterSpec` → the per-column raw predicate string the `col_filters`
/// grammar (`numericFilter.ts` / `parse_numeric_predicate`) expects. `None` when the
/// operand(s) are blank or the op isn't a numeric comparison (`contains` falls back to
/// a substring filter at the call site).
fn numeric_pred_expr(spec: &FilterSpec, val: &str, min: &str, max: &str) -> Option<String> {
    Some(match spec.op {
        FilterOp::Between if !min.is_empty() && !max.is_empty() => format!("{min}..{max}"),
        FilterOp::Gt if !val.is_empty() => format!(">{val}"),
        FilterOp::Gte if !val.is_empty() => format!(">={val}"),
        FilterOp::Lt if !val.is_empty() => format!("<{val}"),
        FilterOp::Lte if !val.is_empty() => format!("<={val}"),
        FilterOp::Eq if !val.is_empty() => format!("={val}"),
        _ => return None,
    })
}

/// Lower a range-capable panel column (dates, lifetime): `between`/one-sided ops fill
/// the inclusive `f[lo_key]`/`f[hi_key]` bounds the panel path evaluates; a `contains`
/// (a DataTable per-column substring on the same column) instead lands as a raw
/// substring `col_filters` entry so that behavior is preserved too.
#[allow(clippy::too_many_arguments)]
fn lower_range(
    f: &mut HashMap<&'static str, String>,
    col_filters: &mut Vec<(String, String)>,
    col_key: &str,
    lo_key: &'static str,
    hi_key: &'static str,
    spec: &FilterSpec,
    val: &str,
    min: &str,
    max: &str,
) {
    match spec.op {
        FilterOp::Between => {
            put_str(f, lo_key, min);
            put_str(f, hi_key, max);
        }
        FilterOp::Gt | FilterOp::Gte => put_str(f, lo_key, val),
        FilterOp::Lt | FilterOp::Lte => put_str(f, hi_key, val),
        // A substring/exact filter on a date/lifetime column → per-column text path.
        FilterOp::Contains | FilterOp::Eq if !val.is_empty() => {
            col_filters.push((col_key.to_string(), val.to_string()))
        }
        _ => {}
    }
}

/// Lower a flag column: the panel's tri-state (`"yes"`/`"no"`) drives the `f` map;
/// any other value (a DataTable substring like `"true"`/`"false"`) stays a per-column
/// text filter.
fn lower_flag(
    f: &mut HashMap<&'static str, String>,
    col_filters: &mut Vec<(String, String)>,
    flag_key: &'static str,
    col_key: &str,
    val: &str,
) {
    match val {
        "yes" | "no" => put_str(f, flag_key, val),
        "" => {}
        other => col_filters.push((col_key.to_string(), other.to_string())),
    }
}

/// Fold one wire `FilterSpec` (keyed by frontend/registry column) onto the internal
/// representation: the panel map `f` for identity/date/lifetime/ix-label/flag columns,
/// or a raw `col_filters` predicate for numeric columns (unknown keys are dropped, so
/// a stale client can't filter everything out). See [`TokenQuery::from_table_request`].
fn lower_filter(
    key: &str,
    spec: &FilterSpec,
    f: &mut HashMap<&'static str, String>,
    col_filters: &mut Vec<(String, String)>,
) {
    let val = operand_str(&spec.val);
    let min = operand_str(&spec.min);
    let max = operand_str(&spec.max);

    match key {
        // Identity: single-field case-insensitive substring (panel semantics).
        "symbol" => put_str(f, "symbol", &val),
        "name" => put_str(f, "name", &val),
        "mint_address" => put_str(f, "mint_address", &val),
        "creator" => put_str(f, "creator", &val),
        "create_tx" => put_str(f, "create_tx", &val),

        // Dates: inclusive [from, to] over a timestamptz column.
        "created" | "created_at" => {
            lower_range(f, col_filters, "created", "created_from", "created_to", spec, &val, &min, &max)
        }
        "last_trade" | "last_trade_at" => {
            lower_range(f, col_filters, "last_trade", "last_trade_from", "last_trade_to", spec, &val, &min, &max)
        }
        "ath_timestamp" => {
            lower_range(f, col_filters, "ath_timestamp", "ath_from", "ath_to", spec, &val, &min, &max)
        }

        // Lifetime (minutes; dead-only stale guard preserved by the panel path).
        "lifetime" => {
            lower_range(f, col_filters, "lifetime", "life_min", "life_max", spec, &val, &min, &max)
        }

        // Instruction labels (JSON ordered-exact vs text-substring grammar).
        "ix_labels" | "ix_label" => put_str(f, "ix_label", &val),

        // Flags: panel tri-state → `f`; DataTable substring → per-column text.
        "migrated" => lower_flag(f, col_filters, "migrated", "migrated", &val),
        "dead" => lower_flag(f, col_filters, "dead", "dead", &val),
        "mayhem_mode" | "mayhem" => lower_flag(f, col_filters, "mayhem", "mayhem_mode", &val),
        "cashback" => lower_flag(f, col_filters, "cashback", "cashback", &val),

        // Everything else: a known numeric column → per-column predicate, else
        // substring; an unknown key is ignored.
        _ => {
            if column(key).is_none() {
                return;
            }
            if let Some(expr) = numeric_pred_expr(spec, &val, &min, &max) {
                col_filters.push((key.to_string(), expr));
            } else if !val.is_empty() {
                col_filters.push((key.to_string(), val));
            }
        }
    }
}

impl TokenQuery {
    /// Sort levels `(column, descending)`, primary first. Exposed for the local
    /// `list_tokens` handler (in the `backend` crate) to detect a chain-sort column
    /// before computing swing stats.
    pub fn sort_levels(&self) -> &[(String, bool)] {
        &self.sort_levels
    }

    /// Swing run id to read chain stats from when a chain column is sorted.
    pub fn swing_run_id(&self) -> Option<&str> {
        self.swing_run_id.as_deref()
    }

    /// Chain-latency budget (ms) used to group those chain stats.
    pub fn swing_chain_latency_ms(&self) -> i64 {
        self.swing_chain_latency_ms
    }

    /// Global-filter value by `TokenFilters` field name, `None` when inactive.
    /// Exposed for the SQL backend (`sql.rs`) to build WHERE clauses from the same
    /// parsed map the in-RAM `matches` reads.
    pub fn f_get(&self, key: &str) -> Option<&str> {
        self.f.get(key).map(String::as_str)
    }

    /// The DataTable global-search string (empty ⇒ inactive).
    pub fn search_str(&self) -> &str {
        &self.search
    }

    /// Parsed per-column `(key, expr)` filters.
    pub fn col_filters_slice(&self) -> &[(String, String)] {
        &self.col_filters
    }

    /// The pasted mint-set (exact, case-sensitive membership over `mint_address`);
    /// empty ⇒ no constraint. Read by the SQL builder to emit `= ANY($n)`.
    pub fn mint_in(&self) -> &[String] {
        &self.mint_in
    }

    /// Lower the unified [`TableRequest`] body onto the internal query. Each
    /// `FilterSpec` in `req.filters` (keyed by frontend/registry column) is folded via
    /// [`lower_filter`] onto EITHER the global panel map `f` (identity / date /
    /// lifetime / ix-label / flag columns, single-field or range semantics) OR a raw
    /// per-column predicate in `col_filters` (numeric columns) — the exact
    /// representation the two eval engines (`matches`, `sql.rs`) already consume. So
    /// the wire is one unified FilterSpec map, evaluation is 100% the proven code.
    /// This is the inverse of the frontend `tokenFiltersToSpecs` + `toTableRequest`.
    pub fn from_table_request(req: &TableRequest) -> Self {
        let mut f: HashMap<&'static str, String> = HashMap::new();
        let mut col_filters: Vec<(String, String)> = Vec::new();
        let mut mint_in: Vec<String> = Vec::new();
        for (key, spec) in &req.filters {
            // The pasted mint-set (`in` op on `mint`) is a set-membership filter, not
            // a substring — lift its array operand here (capped) instead of routing
            // it through `lower_filter` (which handles only single-operand ops).
            if key == "mint_address" && spec.op == crate::api::table_query::FilterOp::In {
                if let Value::Array(arr) = &spec.val {
                    mint_in = arr
                        .iter()
                        .filter_map(|v| match v {
                            Value::String(s) => Some(s.trim().to_string()),
                            Value::Number(n) => Some(n.to_string()),
                            _ => None,
                        })
                        .filter(|s| !s.is_empty())
                        .take(crate::api::table_query::MAX_FILTER_IN_VALUES)
                        .collect();
                }
                continue;
            }
            lower_filter(key, spec, &mut f, &mut col_filters);
        }

        Self {
            search: req.search.clone(),
            sort_levels: sort_levels_from_specs(&req.sorting),
            col_filters,
            mint_in,
            f,
            swing_run_id: req.swing_run_id.clone().filter(|s| !s.is_empty()),
            swing_chain_latency_ms: req.swing_chain_latency_ms.unwrap_or(DEFAULT_CHAIN_LATENCY_MS),
        }
    }

    /// Public wrapper around `matches` for the live SQL handler, which computes the
    /// `tracked` count in-RAM over the cache subset using the SAME predicate the SQL
    /// WHERE reproduces for the full universe.
    pub fn matches_public(&self, t: &TokenSummary, now: DateTime<Utc>) -> bool {
        self.matches(t, now)
    }

    /// `TokenQuery` is `Clone`; this alias documents the intent at the call site
    /// (moving a copy into the blocking `tracked`-count closure).
    pub fn clone_for_tracked(&self) -> Self {
        self.clone()
    }

    /// Public wrapper around `sort_refs` (no swing stats) for the SQL-vs-in-RAM
    /// parity test, so a test can reproduce the in-RAM ordering to diff against the
    /// SQL page.
    pub fn sort_refs_public<'a>(&self, rows: &mut [&'a TokenSummary]) {
        self.sort_refs(rows, None);
    }

    fn matches(&self, t: &TokenSummary, now: DateTime<Utc>) -> bool {
        let f = &self.f;

        // Identity
        if !text_match(&t.symbol, g(f, "symbol")) {
            return false;
        }
        if !text_match(&t.name, g(f, "name")) {
            return false;
        }
        if !text_match(&t.mint_address, g(f, "mint_address")) {
            return false;
        }
        if !text_match(&t.creator_address, g(f, "creator")) {
            return false;
        }
        if !text_match(&t.creation_tx_signature, g(f, "create_tx")) {
            return false;
        }

        // Pasted mint set (exact membership) — mirrors the SQL `= ANY($n)`.
        if !self.mint_in.is_empty() && !self.mint_in.iter().any(|m| m == &t.mint_address) {
            return false;
        }

        // Time
        if !date_in_range(Some(t.created_at), g(f, "created_from"), g(f, "created_to")) {
            return false;
        }
        if !date_in_range(t.last_trade_at, g(f, "last_trade_from"), g(f, "last_trade_to")) {
            return false;
        }
        if !date_in_range(t.ath_timestamp, g(f, "ath_from"), g(f, "ath_to")) {
            return false;
        }

        // Lifetime (minutes): dead tokens only — still-alive/unknown are exempt.
        let (life_min, life_max) = (g(f, "life_min"), g(f, "life_max"));
        if !life_min.is_empty() || !life_max.is_empty() {
            if let Some(life) = lifetime_minutes(t, now) {
                if !range_f64(life, life_min, life_max) {
                    return false;
                }
            }
        }

        // Performance
        if !opt_f64(ath_fep_of(t), g(f, "ath_fep_min"), g(f, "ath_fep_max")) {
            return false;
        }
        if !opt_f64(cur_fep_of(t), g(f, "cur_fep_min"), g(f, "cur_fep_max")) {
            return false;
        }
        if !opt_f64(t.ath_price, g(f, "ath_price_min"), g(f, "ath_price_max")) {
            return false;
        }
        if !opt_f64(t.current_price, g(f, "price_min"), g(f, "price_max")) {
            return false;
        }

        // Market
        if !range_f64(t.volume_sol_total, g(f, "volume_min"), g(f, "volume_max")) {
            return false;
        }
        if !opt_f64(
            t.first_slot_buy_sol,
            g(f, "first_slot_buy_min"),
            g(f, "first_slot_buy_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.first_slot_sell_sol,
            g(f, "first_slot_sell_min"),
            g(f, "first_slot_sell_max"),
        ) {
            return false;
        }
        if !opt_f64(t.market_cap, g(f, "mcap_min"), g(f, "mcap_max")) {
            return false;
        }
        if !range_f64(t.trade_count as f64, g(f, "trades_min"), g(f, "trades_max")) {
            return false;
        }
        if !opt_f64(t.initial_buy_sol, g(f, "init_buy_min"), g(f, "init_buy_max")) {
            return false;
        }
        if !opt_f64(
            t.initial_supply_token.map(|v| v as f64),
            g(f, "init_supply_min"),
            g(f, "init_supply_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.token_amount.map(|v| v as f64),
            g(f, "token_amount_min"),
            g(f, "token_amount_max"),
        ) {
            return false;
        }
        // max_cost_lamports / spendable_lamports_in are lamports; filter in SOL.
        if !opt_f64(
            t.max_cost_lamports.map(|v| v as f64 / 1e9),
            g(f, "max_cost_lamports_min"),
            g(f, "max_cost_lamports_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.spendable_lamports_in.map(|v| v as f64 / 1e9),
            g(f, "spendable_lamports_in_min"),
            g(f, "spendable_lamports_in_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.min_tokens_out.map(|v| v as f64),
            g(f, "min_tokens_out_min"),
            g(f, "min_tokens_out_max"),
        ) {
            return false;
        }

        // Technical
        if !opt_f64(
            t.cu_limit.map(|v| v as f64),
            g(f, "cu_limit_min"),
            g(f, "cu_limit_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.cu_price.map(|v| v as f64),
            g(f, "cu_price_min"),
            g(f, "cu_price_max"),
        ) {
            return false;
        }
        if !range_f64(
            t.ix_labels_count as f64,
            g(f, "ix_count_min"),
            g(f, "ix_count_max"),
        ) {
            return false;
        }
        let ix_label = g(f, "ix_label");
        if !ix_label.is_empty() && !ix_label_matches(ix_label, &t.instruction_labels) {
            return false;
        }

        // Flags
        if !tri_match(t.is_migrated, g(f, "migrated")) {
            return false;
        }
        if !tri_match(t.is_dead, g(f, "dead")) {
            return false;
        }
        if !tri_match(t.is_mayhem_mode, g(f, "mayhem")) {
            return false;
        }
        if !tri_match(t.is_cashback_enabled, g(f, "cashback")) {
            return false;
        }

        // DataTable global search (any column's text).
        if !self.search.is_empty() && !search_match(t, &self.search.to_lowercase()) {
            return false;
        }

        // DataTable per-column filters.
        for (key, expr) in &self.col_filters {
            if !col_filter_matches(key, expr, t) {
                return false;
            }
        }

        true
    }

    /// Re-order a filtered page of snapshot rows in place. The snapshot is already
    /// newest-first, so the default (no sort column) view is a no-op; an explicit
    /// sort uses decorate-sort-undecorate so each row's key is computed ONCE rather
    /// than on every comparison (the old comparator re-derived it — and re-allocated
    /// date strings — O(n log n) times).
    ///
    /// `swing_stats` (when any sort level is a chain column) supplies each mint's
    /// precomputed chain stats; a chain level reads its field via `swing_sort_value`,
    /// and mints absent from the map sort last (matching the dash the frontend
    /// renders for un-analysed tokens). Normal levels key off the `TokenSummary`
    /// field via `sort_key`.
    ///
    /// Multi-level: each row's full key vector is computed ONCE (decorate-sort-
    /// undecorate), then levels compare in priority order, returning on the first
    /// non-equal level. `mint_address` is the final, stable tiebreak so equal rows
    /// keep a deterministic order across pages and refetches.
    fn sort_refs<'a>(
        &self,
        rows: &mut [&'a TokenSummary],
        swing_stats: Option<&HashMap<String, ChainStats>>,
    ) {
        if self.sort_levels.is_empty() {
            return;
        }
        let level_key = |col: &str, t: &TokenSummary| -> SortKey {
            if is_swing_sort_col(col) {
                SortKey::Num(
                    swing_stats
                        .and_then(|m| m.get(&t.mint_address))
                        .map(|s| swing_sort_value(col, s)),
                )
            } else {
                sort_key(col, t)
            }
        };
        let mut keyed: Vec<(Vec<SortKey>, &'a TokenSummary)> = rows
            .iter()
            .map(|&t| {
                let keys = self.sort_levels.iter().map(|(col, _)| level_key(col, t)).collect();
                (keys, t)
            })
            .collect();
        keyed.sort_by(|a, b| {
            for (i, (_, desc)) in self.sort_levels.iter().enumerate() {
                let ord = cmp_keys(&a.0[i], &b.0[i], *desc);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            a.1.mint_address.cmp(&b.1.mint_address)
        });
        for (slot, (_, t)) in rows.iter_mut().zip(keyed) {
            *slot = t;
        }
    }
}

// --- ported helpers (filters.ts) ------------------------------------------

fn fep(t: &TokenSummary) -> Option<f64> {
    let buy = t.initial_buy_sol?;
    let supply = t.initial_supply_token?;
    if supply == 0 {
        return None;
    }
    Some(buy / supply as f64)
}

fn ath_fep_of(t: &TokenSummary) -> Option<f64> {
    match (fep(t), t.ath_price) {
        (Some(e), Some(a)) if e > 0.0 => Some(a / e),
        _ => None,
    }
}

fn cur_fep_of(t: &TokenSummary) -> Option<f64> {
    match (fep(t), t.current_price) {
        (Some(e), Some(c)) if e > 0.0 => Some(c / e),
        _ => None,
    }
}

fn range_f64(val: f64, min: &str, max: &str) -> bool {
    if !min.is_empty() {
        if let Ok(v) = min.parse::<f64>() {
            if val < v {
                return false;
            }
        }
    }
    if !max.is_empty() {
        if let Ok(v) = max.parse::<f64>() {
            if val > v {
                return false;
            }
        }
    }
    true
}

fn opt_f64(opt: Option<f64>, min: &str, max: &str) -> bool {
    if min.is_empty() && max.is_empty() {
        return true;
    }
    match opt {
        Some(v) => range_f64(v, min, max),
        None => false,
    }
}

fn text_match(value: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    value.to_lowercase().contains(needle.trim().to_lowercase().as_str())
}

fn tri_match(value: bool, tri: &str) -> bool {
    match tri {
        "yes" => value,
        "no" => !value,
        _ => true,
    }
}

/// datetime-local value -> UTC instant. The string is treated as a UTC
/// wall-clock: append `:00Z` when seconds are absent (16 chars), else `Z`.
///
/// The frontend now pre-converts the picker value from the selected project
/// timezone to the exact UTC wall-clock before sending it (a 19-char
/// `YYYY-MM-DDTHH:mm:ss`, the non-16-char branch). This contract — "datetime
/// filters are UTC, the client does any tz conversion" — is intentional; keep it
/// when editing. See `datetimeLocalToUtcWallClock` in `frontend-react/utils/date.ts`.
fn parse_dt(v: &str) -> Option<DateTime<Utc>> {
    if v.is_empty() {
        return None;
    }
    let iso = if v.len() == 16 {
        format!("{v}:00Z")
    } else {
        format!("{v}Z")
    };
    DateTime::parse_from_rfc3339(&iso)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn date_in_range(t: Option<DateTime<Utc>>, from: &str, to: &str) -> bool {
    if from.is_empty() && to.is_empty() {
        return true;
    }
    let t = match t {
        Some(t) => t,
        None => return false,
    };
    if let Some(f) = parse_dt(from) {
        if t < f {
            return false;
        }
    }
    if let Some(u) = parse_dt(to) {
        if t > u {
            return false;
        }
    }
    true
}

fn lifetime_minutes(t: &TokenSummary, now: DateTime<Utc>) -> Option<f64> {
    let last = t.last_trade_at?;
    if (now - last).num_milliseconds() < LIFETIME_STALE_MS {
        return None; // still trading → exempt
    }
    if let Some(secs) = t.lifetime_secs {
        return Some(secs as f64 / 60.0);
    }
    Some((last - t.created_at).num_milliseconds() as f64 / 60_000.0)
}

// --- ix_label matching (filters.ts parseIxLabelFilter) ---------------------

enum IxFilter {
    None,
    Text(Vec<String>),
    Json(Vec<String>),
}

/// Token's instruction labels, lowercased. Mirrors JS `String(v)`: bare string
/// for JSON strings, the JSON text otherwise.
fn ix_label_list(value: &Value) -> Vec<String> {
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

fn parse_ix_label_filter(raw: &str) -> IxFilter {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return IxFilter::None;
    }
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            let arr = match &parsed {
                Value::Array(a) => Some(a.clone()),
                Value::Object(o) => match o.get("instructions") {
                    Some(Value::Array(a)) => Some(a.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(a) = arr {
                let needles: Vec<String> = a
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.trim().to_string(),
                        other => other.to_string().trim().to_string(),
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                if !needles.is_empty() {
                    return IxFilter::Json(needles);
                }
            }
        }
        // fall through to text mode
    }
    let needles: Vec<String> = trimmed
        .split(['\n', ','])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if needles.is_empty() {
        IxFilter::None
    } else {
        IxFilter::Text(needles)
    }
}

fn ix_label_matches(raw: &str, value: &Value) -> bool {
    match parse_ix_label_filter(raw) {
        IxFilter::None => true,
        IxFilter::Json(needles) => {
            let labels = ix_label_list(value);
            needles.len() == labels.len()
                && needles
                    .iter()
                    .zip(&labels)
                    .all(|(n, l)| n.to_lowercase() == *l)
        }
        IxFilter::Text(needles) => {
            let labels = ix_label_list(value);
            needles
                .iter()
                .any(|n| labels.iter().any(|l| l.contains(n)))
        }
    }
}

// --- global search ---------------------------------------------------------

fn opt_num_str<T: ToString>(o: Option<T>) -> String {
    o.map(|v| v.to_string()).unwrap_or_default()
}

/// Case-insensitive substring test against a borrowed field. Lowercasing only
/// the candidate (one short allocation per field, and only until a match) keeps
/// the common no-match scan allocation-free for the string fields below.
fn field_contains(field: &str, q_lower: &str) -> bool {
    field.to_lowercase().contains(q_lower)
}

fn search_match(t: &TokenSummary, q_lower: &str) -> bool {
    // Global search is deliberately narrowed to mint + symbol only (locked
    // decision). This drops the old creator-wallet/create-tx/date/numeric matching
    // — and with it the `float8::text` vs Rust `to_string` numeric formatting drift
    // that made the SQL and in-RAM engines disagree on numeric-substring hits. The
    // SQL side (`sql.rs::search_clause`) mirrors this exact field set.
    field_contains(&t.mint_address, q_lower) || field_contains(&t.symbol, q_lower)
}

// --- per-column filters (numericFilter.ts grammar) -------------------------

enum NumPred {
    Range(f64, f64),
    Gt(f64),
    Ge(f64),
    Lt(f64),
    Le(f64),
    Ne(f64),
    Eq(f64),
}

/// Public mirror of `NumPred` for the SQL backend (`sql.rs`), which needs to emit a
/// comparison per variant. Kept as a distinct public type so the private `NumPred`
/// stays an internal detail of the in-RAM evaluator.
pub enum NumPredPublic {
    Range(f64, f64),
    Gt(f64),
    Ge(f64),
    Lt(f64),
    Le(f64),
    Ne(f64),
    Eq(f64),
}

/// Parse a per-column numeric predicate (public wrapper — same grammar as the
/// in-RAM path).
pub fn parse_numeric_predicate_public(text: &str) -> Option<NumPredPublic> {
    parse_numeric_predicate(text).map(|p| match p {
        NumPred::Range(lo, hi) => NumPredPublic::Range(lo, hi),
        NumPred::Gt(v) => NumPredPublic::Gt(v),
        NumPred::Ge(v) => NumPredPublic::Ge(v),
        NumPred::Lt(v) => NumPredPublic::Lt(v),
        NumPred::Le(v) => NumPredPublic::Le(v),
        NumPred::Ne(v) => NumPredPublic::Ne(v),
        NumPred::Eq(v) => NumPredPublic::Eq(v),
    })
}

/// Public mirror of `IxFilter` for the SQL backend.
pub enum IxFilterPublic {
    None,
    Text(Vec<String>),
    Json(Vec<String>),
}

/// Parse an ix-label filter (public wrapper — same grammar as the in-RAM path).
pub fn parse_ix_label_filter_public(raw: &str) -> IxFilterPublic {
    match parse_ix_label_filter(raw) {
        IxFilter::None => IxFilterPublic::None,
        IxFilter::Text(v) => IxFilterPublic::Text(v),
        IxFilter::Json(v) => IxFilterPublic::Json(v),
    }
}

fn parse_numeric_predicate(text: &str) -> Option<NumPred> {
    let t = text.trim();
    if let Some(idx) = t.find("..") {
        let lo = t[..idx].trim().parse::<f64>().ok()?;
        let hi = t[idx + 2..].trim().parse::<f64>().ok()?;
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
        return Some(NumPred::Range(lo, hi));
    }
    // Order matters: multi-char operators before their single-char prefixes.
    for op in [">=", "<=", "==", "!=", ">", "<", "="] {
        if let Some(rest) = t.strip_prefix(op) {
            let v = rest.trim().parse::<f64>().ok()?;
            return Some(match op {
                ">" => NumPred::Gt(v),
                ">=" => NumPred::Ge(v),
                "<" => NumPred::Lt(v),
                "<=" => NumPred::Le(v),
                "!=" => NumPred::Ne(v),
                _ => NumPred::Eq(v),
            });
        }
    }
    None
}

fn eval_pred(p: &NumPred, n: f64) -> bool {
    match *p {
        NumPred::Range(lo, hi) => n >= lo && n <= hi,
        NumPred::Gt(v) => n > v,
        NumPred::Ge(v) => n >= v,
        NumPred::Lt(v) => n < v,
        NumPred::Le(v) => n <= v,
        NumPred::Ne(v) => n != v,
        NumPred::Eq(v) => n == v,
    }
}

/// Numeric value for a column's per-column filter, in displayed units (registry
/// `ram_num`). `None` for unknown / non-numeric keys.
fn col_filter_number(key: &str, t: &TokenSummary) -> Option<f64> {
    column(key).and_then(|c| c.ram_num).and_then(|f| f(t))
}

/// Raw text for a column's per-column substring filter (registry `ram_text`;
/// deviation: raw rather than the JS-formatted value). `""` for unknown keys.
fn col_filter_text(key: &str, t: &TokenSummary) -> String {
    column(key).map(|c| (c.ram_text)(t)).unwrap_or_default()
}

fn col_filter_matches(key: &str, expr: &str, t: &TokenSummary) -> bool {
    let text = expr.trim();
    if text.is_empty() {
        return true;
    }
    if is_numeric_col(key) {
        if let Some(pred) = parse_numeric_predicate(text) {
            return match col_filter_number(key, t) {
                Some(n) => eval_pred(&pred, n),
                None => false,
            };
        }
    }
    col_filter_text(key, t)
        .to_lowercase()
        .contains(text.to_lowercase().as_str())
}

// --- sort (compareSort null/dir semantics) ---------------------------------

enum SortKey {
    Num(Option<f64>),
    Str(Option<String>),
}

/// In-RAM sort key for a column (registry `ram_sort`). Unknown / non-sortable keys
/// fall back to `Str(None)` (sorts last), matching the prior default arm.
fn sort_key(col: &str, t: &TokenSummary) -> SortKey {
    match column(col).and_then(|c| c.ram_sort) {
        Some(f) => f(t),
        None => SortKey::Str(None),
    }
}

/// Mirrors compareSort: nulls always sort last (regardless of dir); numbers
/// numerically; strings case-insensitively.
fn cmp_keys(a: &SortKey, b: &SortKey, desc: bool) -> Ordering {
    let base = match (a, b) {
        (SortKey::Num(x), SortKey::Num(y)) => match (x, y) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(x), Some(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        },
        (SortKey::Str(x), SortKey::Str(y)) => match (x, y) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(x), Some(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        },
        _ => Ordering::Equal,
    };
    if desc {
        base.reverse()
    } else {
        base
    }
}

#[cfg(test)]
mod grammar_parity_tests {
    use super::*;
    use std::collections::BTreeSet;

    // SSOT guard (no DB). The token-list grammar is now ONE registry (`build_registry`)
    // that both engines read, so SQL-vs-in-RAM *key* drift is impossible by
    // construction. These tests instead pin the registry's internal invariants (each
    // column defines BOTH sides of every capability it claims) and freeze the numeric
    // / sortable key sets so a careless registry edit can't silently change the
    // grammar. The DB-backed `token_repo::parity_tests` still proves the resulting
    // *values* agree between the SQL and in-RAM projections.

    #[test]
    fn registry_defines_both_engines_for_every_capability() {
        let mut seen = BTreeSet::new();
        for c in build_registry() {
            assert!(seen.insert(c.key), "duplicate registry key `{}`", c.key);
            // A column claiming to be numeric-filterable must define BOTH the SQL
            // expression and the in-RAM accessor (never one without the other).
            assert_eq!(
                c.sql_num.is_some(),
                c.ram_num.is_some(),
                "column `{}`: sql_num / ram_num presence must match",
                c.key
            );
            assert_eq!(
                c.sql_sort.is_some(),
                c.ram_sort.is_some(),
                "column `{}`: sql_sort / ram_sort presence must match",
                c.key
            );
        }
    }

    #[test]
    fn numeric_grammar_key_set_is_frozen() {
        let numeric: BTreeSet<&str> = build_registry()
            .iter()
            .filter(|c| c.sql_num.is_some())
            .map(|c| c.key)
            .collect();
        let expected: BTreeSet<&str> = [
            "trade_count", "ath_fep_ratio", "current_fep_ratio", "market_cap", "volume",
            "first_slot_buy", "first_slot_sell", "initial_buy", "init_supply", "token_amount",
            "max_cost_lamports", "spendable_lamports_in", "min_tokens_out", "cu_limit", "cu_price",
            "ix_count", "ath_price", "current_price",
        ]
        .into_iter()
        .collect();
        assert_eq!(numeric, expected, "numeric-filter grammar changed unexpectedly");
        // `is_numeric_col` (used by the SQL backend) agrees with the registry.
        for k in &expected {
            assert!(is_numeric_col(k), "is_numeric_col disagrees for `{k}`");
        }
    }

    #[test]
    fn sortable_key_set_is_frozen() {
        let sortable: BTreeSet<&str> = build_registry()
            .iter()
            .filter(|c| c.sql_sort.is_some())
            .map(|c| c.key)
            .collect();
        let expected: BTreeSet<&str> = [
            "symbol", "name", "mint_address", "creator", "token_age", "created", "last_trade", "lifetime",
            "last_synced", "trade_count", "ath_price", "ath_timestamp", "ath_fep_ratio",
            "current_price", "current_fep_ratio", "market_cap", "volume", "first_slot_buy",
            "first_slot_sell", "initial_buy", "init_supply", "token_amount", "max_cost_lamports",
            "spendable_lamports_in", "min_tokens_out", "cu_limit", "cu_price", "ix_count",
            "migrated", "dead", "mayhem_mode", "cashback",
        ]
        .into_iter()
        .collect();
        assert_eq!(sortable, expected, "sortable grammar changed unexpectedly");
        // Swing-chain columns remain browser-derived: sortable-key-accepted but with
        // no registry / SQL sort expression (they fall back to default order in SQL).
        for k in SWING_SORT_COLS {
            assert!(is_sortable_key(k), "swing col `{k}` must be an accepted sort key");
            assert!(sort_sql_expr(k).is_none(), "swing col `{k}` must have no SQL sort expr");
        }
    }
}

#[cfg(test)]
mod lowering_tests {
    use super::*;
    use serde_json::json;

    fn req(json: serde_json::Value) -> TableRequest {
        serde_json::from_value(json).expect("TableRequest")
    }

    #[test]
    fn sorting_specs_resolve_and_drop_unknown() {
        // Multi-key preserved in order + direction; unknown columns dropped.
        let q = TokenQuery::from_table_request(&req(json!({
            "sorting": [{"col":"trade_count","dir":"desc"}, {"col":"bogus","dir":"asc"}, {"col":"symbol","dir":"asc"}]
        })));
        assert_eq!(
            q.sort_levels(),
            &[("trade_count".to_string(), true), ("symbol".to_string(), false)]
        );
    }

    #[test]
    fn numeric_filter_lowers_to_col_predicate() {
        // A numeric column's FilterSpec becomes the raw `col_filters` predicate the
        // registry numeric path consumes (same as the old `cf=trade_count:>=100`).
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"trade_count": {"op":"gte","val":100}}
        })));
        assert_eq!(q.col_filters_slice(), &[("trade_count".to_string(), ">=100".to_string())]);
        assert!(q.f_get("trades_min").is_none(), "numeric filters don't touch the panel map");
    }

    #[test]
    fn between_lowers_to_range_expr() {
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"market_cap": {"op":"between","min":5,"max":50}}
        })));
        assert_eq!(q.col_filters_slice(), &[("market_cap".to_string(), "5..50".to_string())]);
    }

    #[test]
    fn mint_set_in_op_lifts_to_mint_in() {
        // The pasted mint-set `in` op becomes the exact-membership `mint_in` list,
        // NOT the `mint` substring panel filter and NOT a `col_filters` entry.
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"mint_address": {"op":"in","val":["MintA", "MintB", ""]}}
        })));
        assert_eq!(q.mint_in(), &["MintA".to_string(), "MintB".to_string()], "blanks dropped");
        assert!(q.f_get("mint_address").is_none(), "mint-set doesn't touch the substring panel filter");
        assert!(q.col_filters_slice().is_empty(), "mint-set isn't a per-column predicate");
    }

    #[test]
    fn mint_substring_op_still_lowers_to_panel() {
        // A `contains` on `mint` is the substring identity filter, unaffected by the
        // set path.
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"mint_address": {"op":"contains","val":"abc"}}
        })));
        assert_eq!(q.f_get("mint_address"), Some("abc"));
        assert!(q.mint_in().is_empty());
    }

    #[test]
    fn identity_and_flags_and_dates_lower_to_panel_map() {
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {
                "symbol":        {"op":"contains","val":"BONK"},
                "migrated":      {"op":"eq","val":"yes"},
                "created":       {"op":"between","min":"2026-01-01T00:00:00","max":"2026-02-01T00:00:00"},
                "lifetime":      {"op":"lte","val":"60"},
                "ix_labels":     {"op":"contains","val":"buy"}
            }
        })));
        assert_eq!(q.f_get("symbol"), Some("BONK"));
        assert_eq!(q.f_get("migrated"), Some("yes"));
        assert_eq!(q.f_get("created_from"), Some("2026-01-01T00:00:00"));
        assert_eq!(q.f_get("created_to"), Some("2026-02-01T00:00:00"));
        assert_eq!(q.f_get("life_max"), Some("60"));
        assert_eq!(q.f_get("ix_label"), Some("buy"));
        assert!(q.col_filters_slice().is_empty(), "panel-routed filters don't hit col_filters");
    }

    #[test]
    fn flag_substring_stays_a_col_filter() {
        // A DataTable substring on a flag column ("true") is NOT the panel tri-state,
        // so it stays a per-column text filter (unchanged behavior).
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"migrated": {"op":"contains","val":"true"}}
        })));
        assert!(q.f_get("migrated").is_none());
        assert_eq!(q.col_filters_slice(), &[("migrated".to_string(), "true".to_string())]);
    }

    #[test]
    fn unknown_filter_key_is_dropped() {
        let q = TokenQuery::from_table_request(&req(json!({
            "filters": {"bogus_col": {"op":"gt","val":5}}
        })));
        assert!(q.col_filters_slice().is_empty());
        assert!(q.f_get("bogus_col").is_none());
    }

    #[test]
    fn tokens_only_fields_deserialize_camelcase() {
        // Locks the exact camelCase wire shape the frontend POSTs (pagination.pageSize,
        // trackedOnly, swingRunId, swingChainLatencyMs) against the `TableRequest`
        // rename — a silent rename drift would break the live list at runtime.
        let r = req(json!({
            "pagination": {"page": 2, "pageSize": 50},
            "sorting": [{"col":"market_cap","dir":"desc"}],
            "search": "bonk",
            "filters": {"volume": {"op":"between","min":5,"max":50}},
            "trackedOnly": true,
            "swingRunId": "run-123",
            "swingChainLatencyMs": 90000
        }));
        assert_eq!(r.pagination.page, 2);
        assert_eq!(r.pagination.page_size, 50);
        assert!(r.tracked_only);
        let q = TokenQuery::from_table_request(&r);
        assert_eq!(q.swing_run_id(), Some("run-123"));
        assert_eq!(q.swing_chain_latency_ms(), 90_000);
        assert_eq!(q.search_str(), "bonk");
        assert_eq!(q.col_filters_slice(), &[("volume".to_string(), "5..50".to_string())]);
    }
}
