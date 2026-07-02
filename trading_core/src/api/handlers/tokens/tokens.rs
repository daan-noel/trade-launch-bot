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
    state::{core_state::CoreState, token_cache::TokenState},
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
    pub max_sol_cost: Option<u64>,
    pub spendable_sol_in: Option<u64>,
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
    pub create_tx_address: String,
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
            max_sol_cost: extract_buy_arg_u64(&s.token.initial_buy_instruction, "max_sol_cost"),
            spendable_sol_in: extract_buy_arg_u64(
                &s.token.initial_buy_instruction,
                "spendable_sol_in",
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
            create_tx_address: s.token.creation_tx_signature.clone(),
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
            volume_sol_total: r.volume.unwrap_or(0.0),
            first_slot_buy_sol: r.first_slot_buy_sol,
            first_slot_sell_sol: r.first_slot_sell_sol,
            market_cap: r.market_cap,
            initial_buy_sol: r.initial_buy_sol,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            token_amount: extract_buy_arg_u64(&buy_ix, "token_amount"),
            max_sol_cost: extract_buy_arg_u64(&buy_ix, "max_sol_cost"),
            spendable_sol_in: extract_buy_arg_u64(&buy_ix, "spendable_sol_in"),
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
            create_tx_address: r.creation_tx_signature,
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
    pub max_sol_cost: Option<u64>,
    pub spendable_sol_in: Option<u64>,
    pub min_tokens_out: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub is_mayhem_mode: bool,
    pub is_cashback_enabled: bool,
    pub instruction_labels: serde_json::Value,
    pub create_tx_address: String,
    pub created_at: DateTime<Utc>,
    pub trade_count: Option<u64>,
    pub volume_sol_total: Option<f64>,
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
            max_sol_cost: extract_buy_arg_u64(&s.token.initial_buy_instruction, "max_sol_cost"),
            spendable_sol_in: extract_buy_arg_u64(
                &s.token.initial_buy_instruction,
                "spendable_sol_in",
            ),
            min_tokens_out: extract_buy_arg_u64(&s.token.initial_buy_instruction, "min_tokens_out"),
            cu_limit: s.token.cu_limit,
            cu_price: s.token.cu_price,
            is_mayhem_mode: s.token.is_mayhem_mode,
            is_cashback_enabled: s.token.is_cashback_enabled,
            instruction_labels: s.token.instruction_labels.clone(),
            create_tx_address: s.token.creation_tx_signature.clone(),
            created_at: s.token.created_at,
            trade_count: Some(s.trade_count),
            volume_sol_total: Some(s.volume_sol_total),
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

/// Query params for `GET /api/tokens`. `limit`/`offset`/`search` keep their old
/// meaning (the Swing page still calls `?limit=20000&offset=0`); everything else
/// is optional and, when present, drives server-side filtering/sorting that
/// mirrors the React table + global filter panel. Filter fields are a flat
/// mirror of `TokenFilters` (filters.ts) prefixed `f_`, since serde_urlencoded
/// cannot deserialize nested structs.
#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    /// DataTable global search box.
    pub search: Option<String>,

    // Multi-key column sort (DataTable header clicks). `sort` is the current
    // wire format: an ordered, comma-joined `col:dir` list (index 0 = primary),
    // e.g. `sort=trade_count:desc,symbol:asc`. `sort_col`/`sort_dir` are the
    // legacy single-key params, still accepted as a 1-level fallback.
    pub sort: Option<String>,
    pub sort_col: Option<String>,
    pub sort_dir: Option<String>,

    /// Per-column filters, `;`-joined `colKey:rawExpr` entries.
    pub cf: Option<String>,

    // Swing chain sort: when any sort level is a chain column
    // (`swing_pairs` / `max_seq_pairs` / `chain_count`), the order key is computed
    // from the raw legs stashed under `swing_run_id` (the last "Swing Detection
    // All" run), grouped at `swing_chain_latency_ms`. Absent ⇒ those columns sort
    // every row last (no run to read), leaving the default order.
    pub swing_run_id: Option<String>,
    pub swing_chain_latency_ms: Option<i64>,

    // --- Flat TokenFilters mirror (names == TS keys) ---
    pub f_symbol: Option<String>,
    pub f_name: Option<String>,
    pub f_mint: Option<String>,
    pub f_creator: Option<String>,
    pub f_create_tx: Option<String>,
    pub f_created_from: Option<String>,
    pub f_created_to: Option<String>,
    pub f_last_trade_from: Option<String>,
    pub f_last_trade_to: Option<String>,
    pub f_ath_from: Option<String>,
    pub f_ath_to: Option<String>,
    pub f_life_min: Option<String>,
    pub f_life_max: Option<String>,
    pub f_ath_fep_min: Option<String>,
    pub f_ath_fep_max: Option<String>,
    pub f_cur_fep_min: Option<String>,
    pub f_cur_fep_max: Option<String>,
    pub f_ath_price_min: Option<String>,
    pub f_ath_price_max: Option<String>,
    pub f_price_min: Option<String>,
    pub f_price_max: Option<String>,
    pub f_volume_min: Option<String>,
    pub f_volume_max: Option<String>,
    pub f_first_slot_buy_min: Option<String>,
    pub f_first_slot_buy_max: Option<String>,
    pub f_first_slot_sell_min: Option<String>,
    pub f_first_slot_sell_max: Option<String>,
    pub f_mcap_min: Option<String>,
    pub f_mcap_max: Option<String>,
    pub f_trades_min: Option<String>,
    pub f_trades_max: Option<String>,
    pub f_init_buy_min: Option<String>,
    pub f_init_buy_max: Option<String>,
    pub f_init_supply_min: Option<String>,
    pub f_init_supply_max: Option<String>,
    pub f_token_amount_min: Option<String>,
    pub f_token_amount_max: Option<String>,
    pub f_max_sol_cost_min: Option<String>,
    pub f_max_sol_cost_max: Option<String>,
    pub f_spendable_sol_in_min: Option<String>,
    pub f_spendable_sol_in_max: Option<String>,
    pub f_min_tokens_out_min: Option<String>,
    pub f_min_tokens_out_max: Option<String>,
    pub f_cu_limit_min: Option<String>,
    pub f_cu_limit_max: Option<String>,
    pub f_cu_price_min: Option<String>,
    pub f_cu_price_max: Option<String>,
    pub f_ix_count_min: Option<String>,
    pub f_ix_count_max: Option<String>,
    pub f_ix_label: Option<String>,
    pub f_migrated: Option<String>,
    pub f_dead: Option<String>,
    pub f_mayhem: Option<String>,
    pub f_cashback: Option<String>,
    /// When `true`, restrict results to the live cache-tracked subset only
    /// (mirrors the "tracked" badge click on the Tokens page).
    pub tracked_only: Option<bool>,
}

fn default_limit() -> i64 {
    50
}

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
                "max_sol_cost": buy_arg("max_sol_cost"),
                "spendable_sol_in": buy_arg("spendable_sol_in"),
                "min_tokens_out": buy_arg("min_tokens_out"),
                "cu_limit": token.cu_limit,
                "cu_price": token.cu_price,
                "is_mayhem_mode": token.is_mayhem_mode,
                "is_cashback_enabled": token.is_cashback_enabled,
                "instruction_labels": token.ix_labels,
                "create_tx_address": token.creation_tx_signature,
                "created_at": token.created_at,
                "trade_count": token.trade_count,
                "volume_sol_total": token.volume,
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
            "mint": mint
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

/// Columns whose per-column filter understands the numeric grammar (they declare
/// `filterNumber` in tokenColumns.tsx). All others are substring-only.
const NUMERIC_COLS: &[&str] = &[
    "trade_count",
    "ath_fep_ratio",
    "current_fep_ratio",
    "market_cap",
    "volume",
    "first_slot_buy",
    "first_slot_sell",
    "initial_buy",
    "init_supply",
    "token_amount",
    "max_sol_cost",
    "spendable_sol_in",
    "min_tokens_out",
    "cu_limit",
    "cu_price",
    "ix_count",
];

/// Sortable column keys (have a `sortValue` in tokenColumns.tsx).
const SORTABLE_COLS: &[&str] = &[
    "symbol",
    "name",
    "mint",
    "creator",
    "token_age",
    "created",
    "last_trade",
    "lifetime",
    "last_synced",
    "trade_count",
    "ath_price",
    "ath_timestamp",
    "ath_fep_ratio",
    "current_price",
    "current_fep_ratio",
    "market_cap",
    "volume",
    "first_slot_buy",
    "first_slot_sell",
    "initial_buy",
    "init_supply",
    "token_amount",
    "max_sol_cost",
    "spendable_sol_in",
    "min_tokens_out",
    "cu_limit",
    "cu_price",
    "ix_count",
    "migrated",
    "dead",
    "mayhem_mode",
    "cashback",
    // Browser-derived "Swing Detection All" chain columns. Sorted from the raw
    // legs in `state.swing_runs` (see `swing_sort_value`), not a `TokenSummary`
    // field — handled specially in `list_tokens` / `sort_refs`.
    "swing_pairs",
    "max_seq_pairs",
    "chain_count",
];

/// The chain columns whose sort key comes from a swing run rather than a
/// `TokenSummary` field. Default chain latency when the client omits it.
const SWING_SORT_COLS: &[&str] = &["swing_pairs", "max_seq_pairs", "chain_count"];
const DEFAULT_CHAIN_LATENCY_MS: i64 = 60_000;

pub fn is_swing_sort_col(col: &str) -> bool {
    SWING_SORT_COLS.contains(&col)
}

/// Whether a per-column filter key understands the numeric grammar (mirrors
/// `NUMERIC_COLS`). Exposed for the SQL backend.
pub fn is_numeric_col(key: &str) -> bool {
    NUMERIC_COLS.contains(&key)
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
    "(CASE WHEN t.initial_buy_sol IS NOT NULL AND t.initial_supply_token > 0 \
            AND (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) > 0 \
            AND i.ath_price IS NOT NULL \
       THEN i.ath_price / (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) END)"
}

/// current_fep = current_price / entry (entry>0).
pub fn cur_fep_sql_expr() -> &'static str {
    "(CASE WHEN t.initial_buy_sol IS NOT NULL AND t.initial_supply_token > 0 \
            AND (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) > 0 \
            AND i.current_price IS NOT NULL \
       THEN i.current_price / (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) END)"
}

/// SQL numeric expression for a per-column numeric filter key (mirrors
/// `col_filter_number`). `None` for unknown keys.
pub fn col_filter_number_sql(key: &str) -> Option<String> {
    Some(
        match key {
            "trade_count" => "COALESCE(i.trade_count, 0)".into(),
            "ath_fep_ratio" => ath_fep_sql_expr().to_string(),
            "current_fep_ratio" => cur_fep_sql_expr().to_string(),
            "market_cap" => "(i.current_price * t.initial_supply_token)".into(),
            "volume" => "COALESCE(i.volume, 0)".into(),
            "first_slot_buy" => "i.first_slot_buy_sol::float8/1e9".into(),
            "first_slot_sell" => "i.first_slot_sell_sol::float8/1e9".into(),
            "initial_buy" => "t.initial_buy_sol::float8/1e9".into(),
            "init_supply" => "t.initial_supply_token::float8".into(),
            "token_amount" => buy_arg_sql("token_amount"),
            "max_sol_cost" => format!("({}/1e9)", buy_arg_sql("max_sol_cost")),
            "spendable_sol_in" => format!("({}/1e9)", buy_arg_sql("spendable_sol_in")),
            "min_tokens_out" => buy_arg_sql("min_tokens_out"),
            "cu_limit" => "t.cu_limit::float8".into(),
            "cu_price" => "t.cu_price::float8".into(),
            "ix_count" => ix_count_sql().into(),
            _ => return None,
        },
    )
}

/// SQL text expression for a per-column substring filter key (mirrors
/// `col_filter_text`). Rendered close to the Rust string form; numeric/date cols
/// stringify to their raw value. `None` for unknown keys.
pub fn col_filter_text_sql(key: &str) -> Option<String> {
    Some(match key {
        "symbol" => "(t.symbol || ' ' || t.name || ' ' || t.mint_address)".into(),
        "name" => "t.name".into(),
        "mint" => "t.mint_address".into(),
        "creator" => "t.creator_wallet".into(),
        "create_tx" => "t.creation_tx_signature".into(),
        "token_age" => "EXTRACT(EPOCH FROM (now() - t.created_at))::bigint::text".into(),
        "created" => rfc3339_sql("t.created_at"),
        "last_trade" => rfc3339_sql("i.last_trade_at"),
        "lifetime" => "COALESCE(i.lifetime_secs::text, '')".into(),
        "last_synced" => rfc3339_sql("sync.last_synced_at"),
        "trade_count" => "COALESCE(i.trade_count, 0)::text".into(),
        "ath_price" => "COALESCE(i.ath_price::text, '')".into(),
        "ath_timestamp" => rfc3339_sql("i.ath_timestamp"),
        "ath_fep_ratio" => format!("COALESCE(({})::text, '')", ath_fep_sql_expr()),
        "current_price" => "COALESCE(i.current_price::text, '')".into(),
        "current_fep_ratio" => format!("COALESCE(({})::text, '')", cur_fep_sql_expr()),
        "market_cap" => "COALESCE((i.current_price * t.initial_supply_token)::text, '')".into(),
        "volume" => "COALESCE(i.volume, 0)::text".into(),
        "first_slot_buy" => "COALESCE((i.first_slot_buy_sol::float8/1e9)::text, '')".into(),
        "first_slot_sell" => "COALESCE((i.first_slot_sell_sol::float8/1e9)::text, '')".into(),
        "initial_buy" => "COALESCE((t.initial_buy_sol::float8/1e9)::text, '')".into(),
        "init_supply" => "COALESCE(t.initial_supply_token::text, '')".into(),
        "token_amount" => format!("COALESCE(({})::text, '')", buy_arg_sql("token_amount")),
        "max_sol_cost" => format!("COALESCE(({}/1e9)::text, '')", buy_arg_sql("max_sol_cost")),
        "spendable_sol_in" => format!("COALESCE(({}/1e9)::text, '')", buy_arg_sql("spendable_sol_in")),
        "min_tokens_out" => format!("COALESCE(({})::text, '')", buy_arg_sql("min_tokens_out")),
        "cu_limit" => "COALESCE(t.cu_limit::text, '')".into(),
        "cu_price" => "COALESCE(t.cu_price::text, '')".into(),
        "ix_count" => format!("{}::text", ix_count_sql()),
        "migrated" => "COALESCE(i.is_migrated, false)::text".into(),
        "dead" => "COALESCE(i.is_dead, false)::text".into(),
        "mayhem_mode" => "t.is_mayhem_mode::text".into(),
        "cashback" => "t.is_cashback_enabled::text".into(),
        _ => return None,
    })
}

/// SQL sort expression for a sortable column, plus whether it's a text sort (so the
/// caller applies a case-insensitive `LOWER`). Mirrors `sort_key` (which dir/null
/// semantics are applied by the caller via `NULLS LAST`). `None` for unknown /
/// swing-chain columns.
pub fn sort_sql_expr(col: &str) -> Option<(&'static str, bool)> {
    Some(match col {
        "symbol" => ("t.symbol", true),
        "name" => ("t.name", true),
        "mint" => ("t.mint_address", true),
        "creator" => ("t.creator_wallet", true),
        // token_age sorts by age = now-created; monotonic with created_at DESC, so
        // sort by created_at with reversed sense handled at caller: age ASC == created DESC.
        // To keep it simple & correct we sort on the numeric age expression.
        "token_age" => ("EXTRACT(EPOCH FROM (now() - t.created_at))", false),
        // in-RAM sorts created/last_trade/ath_timestamp/last_synced as RFC3339
        // STRINGS; lexical order of RFC3339 == chronological, so sort on the ts.
        "created" => ("t.created_at", false),
        "last_trade" => ("i.last_trade_at", false),
        "lifetime" => ("i.lifetime_secs", false),
        "last_synced" => ("sync.last_synced_at", false),
        "trade_count" => ("COALESCE(i.trade_count, 0)", false),
        "ath_price" => ("i.ath_price", false),
        "ath_timestamp" => ("i.ath_timestamp", false),
        "ath_fep_ratio" => (ATH_FEP_SORT, false),
        "current_price" => ("i.current_price", false),
        "current_fep_ratio" => (CUR_FEP_SORT, false),
        "market_cap" => ("(i.current_price * t.initial_supply_token)", false),
        "volume" => ("COALESCE(i.volume, 0)", false),
        "first_slot_buy" => ("i.first_slot_buy_sol", false),
        "first_slot_sell" => ("i.first_slot_sell_sol", false),
        "initial_buy" => ("t.initial_buy_sol", false),
        "init_supply" => ("t.initial_supply_token", false),
        "token_amount" => (TOKEN_AMOUNT_SORT, false),
        "max_sol_cost" => (MAX_SOL_COST_SORT, false),
        "spendable_sol_in" => (SPENDABLE_SORT, false),
        "min_tokens_out" => (MIN_TOKENS_OUT_SORT, false),
        "cu_limit" => ("t.cu_limit", false),
        "cu_price" => ("t.cu_price", false),
        "ix_count" => (IX_COUNT_SORT, false),
        "migrated" => ("(COALESCE(i.is_migrated, false))::int", false),
        "dead" => ("(COALESCE(i.is_dead, false))::int", false),
        "mayhem_mode" => ("t.is_mayhem_mode::int", false),
        "cashback" => ("t.is_cashback_enabled::int", false),
        _ => return None,
    })
}

// Sort-expression constants that need a JSON/CASE body inline as a `&'static str`.
const ATH_FEP_SORT: &str = "(CASE WHEN t.initial_buy_sol IS NOT NULL AND t.initial_supply_token > 0 AND (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) > 0 AND i.ath_price IS NOT NULL THEN i.ath_price / (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) END)";
const CUR_FEP_SORT: &str = "(CASE WHEN t.initial_buy_sol IS NOT NULL AND t.initial_supply_token > 0 AND (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) > 0 AND i.current_price IS NOT NULL THEN i.current_price / (t.initial_buy_sol::float8/1e9 / t.initial_supply_token) END)";
const TOKEN_AMOUNT_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'token_amount' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'token_amount')::float8 END)";
const MAX_SOL_COST_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'max_sol_cost' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'max_sol_cost')::float8 END)";
const SPENDABLE_SORT: &str = "(CASE WHEN t.initial_buy_instruction->>'spendable_sol_in' ~ '^[0-9]+$' THEN (t.initial_buy_instruction->>'spendable_sol_in')::float8 END)";
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

/// Parse the DataTable sort into validated `(column, descending)` levels, index
/// 0 = primary. Prefers the multi-key `sort=col:dir,col:dir` param; falls back
/// to the legacy single `sort_col`/`sort_dir` pair. Unknown columns are dropped
/// (not fatal), so a stale client never breaks the listing.
fn parse_sort_levels(q: &PaginationParams) -> Vec<(String, bool)> {
    sort_levels_from(q.sort.as_deref(), q.sort_col.as_deref(), q.sort_dir.as_deref())
}

/// Core of [`parse_sort_levels`], split out so it's unit-testable without
/// building the ~60-field `PaginationParams`.
fn sort_levels_from(
    sort: Option<&str>,
    sort_col: Option<&str>,
    sort_dir: Option<&str>,
) -> Vec<(String, bool)> {
    if let Some(spec) = sort.filter(|s| !s.is_empty()) {
        return spec
            .split(',')
            .filter_map(|entry| {
                let (col, dir) = entry.split_once(':').unwrap_or((entry, "asc"));
                let col = col.trim();
                if !SORTABLE_COLS.contains(&col) {
                    return None;
                }
                Some((col.to_string(), dir.trim().eq_ignore_ascii_case("desc")))
            })
            .collect();
    }
    // Legacy single-pair fallback.
    match sort_col.filter(|s| SORTABLE_COLS.contains(s)) {
        Some(col) => vec![(col.to_string(), sort_dir == Some("desc"))],
        None => Vec::new(),
    }
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
    /// global filter values keyed by TokenFilters field name (non-empty only)
    f: HashMap<&'static str, String>,
    /// Swing run to read chain stats from when sorting a chain column.
    swing_run_id: Option<String>,
    /// Chain-latency budget (ms) used to group those stats.
    swing_chain_latency_ms: i64,
}

fn put(f: &mut HashMap<&'static str, String>, k: &'static str, v: &Option<String>) {
    if let Some(s) = v {
        if !s.is_empty() {
            f.insert(k, s.clone());
        }
    }
}

/// Look up a global-filter value (defaults to "" — i.e. inactive).
fn g<'a>(f: &'a HashMap<&'static str, String>, k: &str) -> &'a str {
    f.get(k).map(String::as_str).unwrap_or("")
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

    pub fn from_params(q: &PaginationParams) -> Self {
        let mut f: HashMap<&'static str, String> = HashMap::new();
        put(&mut f, "symbol", &q.f_symbol);
        put(&mut f, "name", &q.f_name);
        put(&mut f, "mint", &q.f_mint);
        put(&mut f, "creator", &q.f_creator);
        put(&mut f, "create_tx", &q.f_create_tx);
        put(&mut f, "created_from", &q.f_created_from);
        put(&mut f, "created_to", &q.f_created_to);
        put(&mut f, "last_trade_from", &q.f_last_trade_from);
        put(&mut f, "last_trade_to", &q.f_last_trade_to);
        put(&mut f, "ath_from", &q.f_ath_from);
        put(&mut f, "ath_to", &q.f_ath_to);
        put(&mut f, "life_min", &q.f_life_min);
        put(&mut f, "life_max", &q.f_life_max);
        put(&mut f, "ath_fep_min", &q.f_ath_fep_min);
        put(&mut f, "ath_fep_max", &q.f_ath_fep_max);
        put(&mut f, "cur_fep_min", &q.f_cur_fep_min);
        put(&mut f, "cur_fep_max", &q.f_cur_fep_max);
        put(&mut f, "ath_price_min", &q.f_ath_price_min);
        put(&mut f, "ath_price_max", &q.f_ath_price_max);
        put(&mut f, "price_min", &q.f_price_min);
        put(&mut f, "price_max", &q.f_price_max);
        put(&mut f, "volume_min", &q.f_volume_min);
        put(&mut f, "volume_max", &q.f_volume_max);
        put(&mut f, "first_slot_buy_min", &q.f_first_slot_buy_min);
        put(&mut f, "first_slot_buy_max", &q.f_first_slot_buy_max);
        put(&mut f, "first_slot_sell_min", &q.f_first_slot_sell_min);
        put(&mut f, "first_slot_sell_max", &q.f_first_slot_sell_max);
        put(&mut f, "mcap_min", &q.f_mcap_min);
        put(&mut f, "mcap_max", &q.f_mcap_max);
        put(&mut f, "trades_min", &q.f_trades_min);
        put(&mut f, "trades_max", &q.f_trades_max);
        put(&mut f, "init_buy_min", &q.f_init_buy_min);
        put(&mut f, "init_buy_max", &q.f_init_buy_max);
        put(&mut f, "init_supply_min", &q.f_init_supply_min);
        put(&mut f, "init_supply_max", &q.f_init_supply_max);
        put(&mut f, "token_amount_min", &q.f_token_amount_min);
        put(&mut f, "token_amount_max", &q.f_token_amount_max);
        put(&mut f, "max_sol_cost_min", &q.f_max_sol_cost_min);
        put(&mut f, "max_sol_cost_max", &q.f_max_sol_cost_max);
        put(&mut f, "spendable_sol_in_min", &q.f_spendable_sol_in_min);
        put(&mut f, "spendable_sol_in_max", &q.f_spendable_sol_in_max);
        put(&mut f, "min_tokens_out_min", &q.f_min_tokens_out_min);
        put(&mut f, "min_tokens_out_max", &q.f_min_tokens_out_max);
        put(&mut f, "cu_limit_min", &q.f_cu_limit_min);
        put(&mut f, "cu_limit_max", &q.f_cu_limit_max);
        put(&mut f, "cu_price_min", &q.f_cu_price_min);
        put(&mut f, "cu_price_max", &q.f_cu_price_max);
        put(&mut f, "ix_count_min", &q.f_ix_count_min);
        put(&mut f, "ix_count_max", &q.f_ix_count_max);
        put(&mut f, "ix_label", &q.f_ix_label);
        put(&mut f, "migrated", &q.f_migrated);
        put(&mut f, "dead", &q.f_dead);
        put(&mut f, "mayhem", &q.f_mayhem);
        put(&mut f, "cashback", &q.f_cashback);

        Self {
            search: q.search.clone().unwrap_or_default(),
            sort_levels: parse_sort_levels(q),
            col_filters: q.cf.as_deref().map(parse_col_filters).unwrap_or_default(),
            f,
            swing_run_id: q.swing_run_id.clone().filter(|s| !s.is_empty()),
            swing_chain_latency_ms: q.swing_chain_latency_ms.unwrap_or(DEFAULT_CHAIN_LATENCY_MS),
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
        if !text_match(&t.mint_address, g(f, "mint")) {
            return false;
        }
        if !text_match(&t.creator_address, g(f, "creator")) {
            return false;
        }
        if !text_match(&t.create_tx_address, g(f, "create_tx")) {
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
        // max_sol_cost / spendable_sol_in are lamports; filter in SOL.
        if !opt_f64(
            t.max_sol_cost.map(|v| v as f64 / 1e9),
            g(f, "max_sol_cost_min"),
            g(f, "max_sol_cost_max"),
        ) {
            return false;
        }
        if !opt_f64(
            t.spendable_sol_in.map(|v| v as f64 / 1e9),
            g(f, "spendable_sol_in_min"),
            g(f, "spendable_sol_in_max"),
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
    // Cheap path: match the borrowed string fields with no upfront cloning.
    let str_fields = [
        t.symbol.as_str(),
        t.name.as_str(),
        t.mint_address.as_str(),
        t.creator_address.as_str(),
        t.create_tx_address.as_str(),
    ];
    if str_fields.iter().any(|s| field_contains(s, q_lower)) {
        return true;
    }

    // Only allocate/format the timestamp + numeric fields if nothing above hit.
    let dates = [
        Some(t.created_at),
        t.last_trade_at,
        t.ath_timestamp,
        t.last_synced_at,
    ];
    if dates
        .into_iter()
        .flatten()
        .any(|d| field_contains(&d.to_rfc3339(), q_lower))
    {
        return true;
    }

    if field_contains(&t.trade_count.to_string(), q_lower)
        || field_contains(&t.volume_sol_total.to_string(), q_lower)
    {
        return true;
    }

    let opt_nums = [t.ath_price, t.current_price, t.market_cap];
    opt_nums
        .into_iter()
        .flatten()
        .any(|n| field_contains(&n.to_string(), q_lower))
}

// --- per-column filters (numericFilter.ts grammar) -------------------------

fn parse_col_filters(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (k, v) = part.split_once(':')?;
            let v = v.trim();
            if v.is_empty() {
                return None;
            }
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

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

/// Numeric value for a column's per-column filter, in displayed units.
fn col_filter_number(key: &str, t: &TokenSummary) -> Option<f64> {
    match key {
        "trade_count" => Some(t.trade_count as f64),
        "ath_fep_ratio" => ath_fep_of(t),
        "current_fep_ratio" => cur_fep_of(t),
        "market_cap" => t.market_cap,
        "volume" => Some(t.volume_sol_total),
        "first_slot_buy" => t.first_slot_buy_sol,
        "first_slot_sell" => t.first_slot_sell_sol,
        "initial_buy" => t.initial_buy_sol,
        "init_supply" => t.initial_supply_token.map(|v| v as f64),
        "token_amount" => t.token_amount.map(|v| v as f64),
        "max_sol_cost" => t.max_sol_cost.map(|v| v as f64 / 1e9),
        "spendable_sol_in" => t.spendable_sol_in.map(|v| v as f64 / 1e9),
        "min_tokens_out" => t.min_tokens_out.map(|v| v as f64),
        "cu_limit" => t.cu_limit.map(|v| v as f64),
        "cu_price" => t.cu_price.map(|v| v as f64),
        "ix_count" => Some(t.ix_labels_count as f64),
        _ => None,
    }
}

/// Raw text for a column's per-column substring filter (deviation: raw rather
/// than the JS-formatted value).
fn col_filter_text(key: &str, t: &TokenSummary) -> String {
    match key {
        "symbol" => format!("{} {} {}", t.symbol, t.name, t.mint_address),
        "name" => t.name.clone(),
        "mint" => t.mint_address.clone(),
        "creator" => t.creator_address.clone(),
        "create_tx" => t.create_tx_address.clone(),
        "token_age" => t.age_seconds.to_string(),
        "created" => t.created_at.to_rfc3339(),
        "last_trade" => opt_num_str(t.last_trade_at.map(|d| d.to_rfc3339())),
        "lifetime" => opt_num_str(t.lifetime_secs),
        "last_synced" => opt_num_str(t.last_synced_at.map(|d| d.to_rfc3339())),
        "trade_count" => t.trade_count.to_string(),
        "ath_price" => opt_num_str(t.ath_price),
        "ath_timestamp" => opt_num_str(t.ath_timestamp.map(|d| d.to_rfc3339())),
        "ath_fep_ratio" => opt_num_str(ath_fep_of(t)),
        "current_price" => opt_num_str(t.current_price),
        "current_fep_ratio" => opt_num_str(cur_fep_of(t)),
        "market_cap" => opt_num_str(t.market_cap),
        "volume" => t.volume_sol_total.to_string(),
        "first_slot_buy" => opt_num_str(t.first_slot_buy_sol),
        "first_slot_sell" => opt_num_str(t.first_slot_sell_sol),
        "initial_buy" => opt_num_str(t.initial_buy_sol),
        "init_supply" => opt_num_str(t.initial_supply_token),
        "token_amount" => opt_num_str(t.token_amount),
        "max_sol_cost" => opt_num_str(t.max_sol_cost.map(|v| v as f64 / 1e9)),
        "spendable_sol_in" => opt_num_str(t.spendable_sol_in.map(|v| v as f64 / 1e9)),
        "min_tokens_out" => opt_num_str(t.min_tokens_out),
        "cu_limit" => opt_num_str(t.cu_limit),
        "cu_price" => opt_num_str(t.cu_price),
        "ix_count" => t.ix_labels_count.to_string(),
        "ix_labels" => ix_label_list(&t.instruction_labels).join(", "),
        "migrated" => t.is_migrated.to_string(),
        "dead" => t.is_dead.to_string(),
        "mayhem_mode" => t.is_mayhem_mode.to_string(),
        "cashback" => t.is_cashback_enabled.to_string(),
        _ => String::new(),
    }
}

fn col_filter_matches(key: &str, expr: &str, t: &TokenSummary) -> bool {
    let text = expr.trim();
    if text.is_empty() {
        return true;
    }
    if NUMERIC_COLS.contains(&key) {
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

fn sort_key(col: &str, t: &TokenSummary) -> SortKey {
    match col {
        "symbol" => SortKey::Str(Some(t.symbol.clone())),
        "name" => SortKey::Str(Some(t.name.clone())),
        "mint" => SortKey::Str(Some(t.mint_address.clone())),
        "creator" => SortKey::Str(Some(t.creator_address.clone())),
        "token_age" => SortKey::Num(Some(t.age_seconds as f64)),
        "created" => SortKey::Str(Some(t.created_at.to_rfc3339())),
        "last_trade" => SortKey::Str(t.last_trade_at.map(|d| d.to_rfc3339())),
        "lifetime" => SortKey::Num(t.lifetime_secs.map(|v| v as f64)),
        "last_synced" => SortKey::Str(t.last_synced_at.map(|d| d.to_rfc3339())),
        "trade_count" => SortKey::Num(Some(t.trade_count as f64)),
        "ath_price" => SortKey::Num(t.ath_price),
        "ath_timestamp" => SortKey::Str(t.ath_timestamp.map(|d| d.to_rfc3339())),
        "ath_fep_ratio" => SortKey::Num(ath_fep_of(t)),
        "current_price" => SortKey::Num(t.current_price),
        "current_fep_ratio" => SortKey::Num(cur_fep_of(t)),
        "market_cap" => SortKey::Num(t.market_cap),
        "volume" => SortKey::Num(Some(t.volume_sol_total)),
        "first_slot_buy" => SortKey::Num(t.first_slot_buy_sol),
        "first_slot_sell" => SortKey::Num(t.first_slot_sell_sol),
        "initial_buy" => SortKey::Num(t.initial_buy_sol),
        "init_supply" => SortKey::Num(t.initial_supply_token.map(|v| v as f64)),
        "token_amount" => SortKey::Num(t.token_amount.map(|v| v as f64)),
        // raw lamports — monotonic with /1e9, so equivalent to the displayed sort.
        "max_sol_cost" => SortKey::Num(t.max_sol_cost.map(|v| v as f64)),
        "spendable_sol_in" => SortKey::Num(t.spendable_sol_in.map(|v| v as f64)),
        "min_tokens_out" => SortKey::Num(t.min_tokens_out.map(|v| v as f64)),
        "cu_limit" => SortKey::Num(t.cu_limit.map(|v| v as f64)),
        "cu_price" => SortKey::Num(t.cu_price.map(|v| v as f64)),
        "ix_count" => SortKey::Num(Some(t.ix_labels_count as f64)),
        "migrated" => SortKey::Num(Some(if t.is_migrated { 1.0 } else { 0.0 })),
        "dead" => SortKey::Num(Some(if t.is_dead { 1.0 } else { 0.0 })),
        "mayhem_mode" => SortKey::Num(Some(if t.is_mayhem_mode { 1.0 } else { 0.0 })),
        "cashback" => SortKey::Num(Some(if t.is_cashback_enabled { 1.0 } else { 0.0 })),
        _ => SortKey::Str(None),
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
mod sort_tests {
    use super::sort_levels_from;

    #[test]
    fn multi_key_preserves_order_and_dir() {
        // The reported bug: secondary key was dropped. Both levels must survive,
        // in order, with per-level direction.
        let levels = sort_levels_from(Some("trade_count:desc,symbol:asc"), None, None);
        assert_eq!(
            levels,
            vec![("trade_count".to_string(), true), ("symbol".to_string(), false)]
        );
    }

    #[test]
    fn unknown_columns_are_dropped_not_fatal() {
        let levels = sort_levels_from(Some("bogus:desc,trade_count:asc"), None, None);
        assert_eq!(levels, vec![("trade_count".to_string(), false)]);
    }

    #[test]
    fn missing_dir_defaults_to_asc() {
        let levels = sort_levels_from(Some("symbol"), None, None);
        assert_eq!(levels, vec![("symbol".to_string(), false)]);
    }

    #[test]
    fn legacy_single_pair_fallback() {
        // Old clients / bookmarked URLs without the `sort=` param still work.
        let levels = sort_levels_from(None, Some("market_cap"), Some("desc"));
        assert_eq!(levels, vec![("market_cap".to_string(), true)]);
        // Unknown legacy column → no sort.
        assert!(sort_levels_from(None, Some("bogus"), Some("desc")).is_empty());
    }

    #[test]
    fn empty_sort_is_no_op() {
        assert!(sort_levels_from(Some(""), None, None).is_empty());
        assert!(sort_levels_from(None, None, None).is_empty());
    }
}
