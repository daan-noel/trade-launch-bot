use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    state::{app_state::AppState, token_cache::TokenState},
    storage::repositories::{token_repo::TokenRepo, trade_repo::TradeRepo},
};

fn extract_buy_arg_u64(value: &Option<Value>, field: &str) -> Option<u64> {
    value
        .as_ref()
        .and_then(|obj| obj.get(field))
        .and_then(|v| v.as_u64())
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Compact token summary emitted in list responses.
#[derive(Serialize)]
pub struct TokenSummary {
    pub mint_address: String,
    pub symbol: String,
    pub current_price: Option<f64>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<DateTime<Utc>>,
    pub volume_sol_total: f64,
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
    #[serde(rename = "age")]
    pub age_seconds: i64,
    pub created_at: DateTime<Utc>,
    pub creator_address: String,
    pub create_tx_address: String,
    pub name: String,
    pub trade_count: u64,
    pub last_trade_at: Option<DateTime<Utc>>,
    /// Gap-aware lifetime in seconds (creation → last non-stray trade).
    pub active_lifetime_secs: Option<i64>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl From<&TokenState> for TokenSummary {
    fn from(s: &TokenState) -> Self {
        let age_seconds = chrono::Utc::now()
            .signed_duration_since(s.token.created_at)
            .num_seconds();

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
            age_seconds,
            created_at: s.token.created_at,
            creator_address: s.token.creator_wallet.clone(),
            create_tx_address: s.token.creation_tx_signature.clone(),
            name: s.token.name.clone(),
            trade_count: s.trade_count,
            last_trade_at: s.last_trade_at,
            active_lifetime_secs: s.active_lifetime_secs(),
            last_synced_at: s.last_synced_at,
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
    pub total: usize,
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

    // Column sort (DataTable header click).
    pub sort_col: Option<String>,
    pub sort_dir: Option<String>,

    /// Per-column filters, `;`-joined `colKey:rawExpr` entries.
    pub cf: Option<String>,

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
    pub f_mayhem: Option<String>,
    pub f_cashback: Option<String>,
}

fn default_limit() -> i64 {
    50
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/tokens` — list all currently tracked tokens sorted by trade count.
pub async fn list_tokens(
    state: web::Data<Arc<AppState>>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    // Cloning + sorting + filtering the whole token cache is CPU work that would
    // otherwise block one of the few (http_workers=2) async request threads.
    // Run it on the blocking pool so a large cache can't stall other requests.
    let state = state.get_ref().clone();
    let q = TokenQuery::from_params(&query);
    let limit_q = query.limit;
    let offset_q = query.offset;

    let built = web::block(move || {
        let now = chrono::Utc::now();
        let mut tokens: Vec<TokenSummary> = state
            .token_cache
            .iter()
            .map(|entry| TokenSummary::from(entry.value()))
            .collect();

        // Full-fidelity server-side reduction: global filters + search + per-
        // column filters (mirrors `tokenPassesFilters` and the DataTable).
        tokens.retain(|t| q.matches(t, now));

        // `total` is the FILTERED count — that's what the table's pager needs.
        let total = tokens.len();

        // Sort by the requested column (default: newest created_at first), then
        // page. Sorting precedes paging; the rest of the reduction order is
        // irrelevant to the resulting set.
        q.sort(&mut tokens);

        let limit = limit_q.max(1).min(50_000) as usize;
        let offset = offset_q.max(0) as usize;
        let items: Vec<_> = tokens.into_iter().skip(offset).take(limit).collect();
        TokensListResponse { total, items }
    })
    .await;

    match built {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            tracing::error!("list_tokens blocking build failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "failed to build token list" }))
        }
    }
}

/// `GET /api/tokens/:mint` — token detail from in-memory cache; falls back to DB.
pub async fn get_token(state: web::Data<Arc<AppState>>, path: web::Path<String>) -> impl Responder {
    let mint = path.into_inner();

    // Fast path: served from cache
    if let Some(entry) = state.token_cache.get(&mint) {
        return HttpResponse::Ok().json(TokenDetail::from(entry.value()));
    }

    // Slow path: token was created before this server session
    let repo = TokenRepo::new(state.db.clone());
    match repo.find_by_mint(&mint).await {
        Ok(Some(token)) => {
            // Return a minimal detail (no live stats — token isn't tracked)
            HttpResponse::Ok().json(serde_json::json!({
                "mint_address": token.mint_address,
                "name": token.name,
                "symbol": token.symbol,
                "creator_address": token.creator_wallet,
                "bonding_curve_address": token.bonding_curve_address,
                "initial_supply_token": token.initial_supply_token,
                "initial_buy_sol": token.initial_buy_sol,
                "token_amount": token.initial_buy_instruction.as_ref().and_then(|ix| ix.get("token_amount")).and_then(|v| v.as_u64()),
                "max_sol_cost": token.initial_buy_instruction.as_ref().and_then(|ix| ix.get("max_sol_cost")).and_then(|v| v.as_u64()),
                "spendable_sol_in": token.initial_buy_instruction.as_ref().and_then(|ix| ix.get("spendable_sol_in")).and_then(|v| v.as_u64()),
                "min_tokens_out": token.initial_buy_instruction.as_ref().and_then(|ix| ix.get("min_tokens_out")).and_then(|v| v.as_u64()),
                "cu_limit": token.cu_limit,
                "cu_price": token.cu_price,
                "is_mayhem_mode": token.is_mayhem_mode,
                "is_cashback_enabled": token.is_cashback_enabled,
                "instruction_labels": token.instruction_labels,
                "create_tx_address": token.creation_tx_signature,
                "created_at": token.created_at,
                "trade_count": null,
                "volume_sol_total": null,
                "market_cap": null,
                "current_price": null,
                "ath_price": null,
                "ath_timestamp": null,
                "is_migrated": false,
                "unique_wallets": null,
                "last_trade_at": null,
                "last_synced_at": null,
                "note": "token exists in DB but is not actively tracked this session"
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

/// `GET /api/tokens/:mint/trades`
///
/// Returns trades for a token in chronological order (cache first, else DB),
/// bounded by `limit` (default & cap 5000) and `offset`.
pub async fn get_trades(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    query: web::Query<TradesPageParams>,
) -> impl Responder {
    let mint = path.into_inner();
    let limit = query.limit.clamp(1, 5_000);
    let offset = query.offset.max(0);

    if let Some(entry) = state.token_cache.get(&mint) {
        let page: Vec<_> = entry
            .trades
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect();
        return HttpResponse::Ok().json(page);
    }

    let repo = TradeRepo::new(state.db.clone());
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

/// Inactivity window after which a token is treated as dead (mirrors filters.ts
/// `LIFETIME_STALE_MS` and the backend's RUGGED_STALE_SECONDS = 3600).
const LIFETIME_STALE_MS: i64 = 60 * 60 * 1000;

/// Columns whose per-column filter understands the numeric grammar (they declare
/// `filterNumber` in tokenColumns.tsx). All others are substring-only.
const NUMERIC_COLS: &[&str] = &[
    "trade_count",
    "ath_fep_ratio",
    "current_fep_ratio",
    "market_cap",
    "volume",
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
    "mayhem_mode",
    "cashback",
];

/// Parsed query: global filters + search + per-column filters + sort.
struct TokenQuery {
    search: String,
    sort_col: Option<String>,
    sort_desc: bool,
    /// (column key, raw expression)
    col_filters: Vec<(String, String)>,
    /// global filter values keyed by TokenFilters field name (non-empty only)
    f: HashMap<&'static str, String>,
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
    fn from_params(q: &PaginationParams) -> Self {
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
        put(&mut f, "mayhem", &q.f_mayhem);
        put(&mut f, "cashback", &q.f_cashback);

        Self {
            search: q.search.clone().unwrap_or_default(),
            sort_col: q
                .sort_col
                .clone()
                .filter(|s| SORTABLE_COLS.contains(&s.as_str())),
            sort_desc: q.sort_dir.as_deref() == Some("desc"),
            col_filters: q.cf.as_deref().map(parse_col_filters).unwrap_or_default(),
            f,
        }
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

    fn sort(&self, tokens: &mut [TokenSummary]) {
        match &self.sort_col {
            Some(col) => {
                let desc = self.sort_desc;
                tokens.sort_by(|a, b| {
                    cmp_keys(&sort_key(col, a), &sort_key(col, b), desc)
                });
            }
            // Default: newest created_at first (matches prior behavior).
            None => tokens.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
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

/// datetime-local value -> UTC instant (mirrors filters.ts `parseDt`: append
/// `:00Z` when seconds are absent, else `Z`).
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
    if let Some(secs) = t.active_lifetime_secs {
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

fn search_match(t: &TokenSummary, q_lower: &str) -> bool {
    let fields = [
        t.symbol.clone(),
        t.name.clone(),
        t.mint_address.clone(),
        t.creator_address.clone(),
        t.create_tx_address.clone(),
        t.created_at.to_rfc3339(),
        opt_num_str(t.last_trade_at.map(|d| d.to_rfc3339())),
        opt_num_str(t.ath_timestamp.map(|d| d.to_rfc3339())),
        opt_num_str(t.last_synced_at.map(|d| d.to_rfc3339())),
        t.trade_count.to_string(),
        opt_num_str(t.ath_price),
        opt_num_str(t.current_price),
        t.volume_sol_total.to_string(),
        opt_num_str(t.market_cap),
    ];
    fields.iter().any(|s| s.to_lowercase().contains(q_lower))
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
        "lifetime" => opt_num_str(t.active_lifetime_secs),
        "last_synced" => opt_num_str(t.last_synced_at.map(|d| d.to_rfc3339())),
        "trade_count" => t.trade_count.to_string(),
        "ath_price" => opt_num_str(t.ath_price),
        "ath_timestamp" => opt_num_str(t.ath_timestamp.map(|d| d.to_rfc3339())),
        "ath_fep_ratio" => opt_num_str(ath_fep_of(t)),
        "current_price" => opt_num_str(t.current_price),
        "current_fep_ratio" => opt_num_str(cur_fep_of(t)),
        "market_cap" => opt_num_str(t.market_cap),
        "volume" => t.volume_sol_total.to_string(),
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
        "lifetime" => SortKey::Num(t.active_lifetime_secs.map(|v| v as f64)),
        "last_synced" => SortKey::Str(t.last_synced_at.map(|d| d.to_rfc3339())),
        "trade_count" => SortKey::Num(Some(t.trade_count as f64)),
        "ath_price" => SortKey::Num(t.ath_price),
        "ath_timestamp" => SortKey::Str(t.ath_timestamp.map(|d| d.to_rfc3339())),
        "ath_fep_ratio" => SortKey::Num(ath_fep_of(t)),
        "current_price" => SortKey::Num(t.current_price),
        "current_fep_ratio" => SortKey::Num(cur_fep_of(t)),
        "market_cap" => SortKey::Num(t.market_cap),
        "volume" => SortKey::Num(Some(t.volume_sol_total)),
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
