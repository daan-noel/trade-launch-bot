use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub search: Option<String>,
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
    let search = query.search.clone();
    let limit_q = query.limit;
    let offset_q = query.offset;

    let built = web::block(move || {
        let mut tokens: Vec<TokenSummary> = state
            .token_cache
            .iter()
            .map(|entry| TokenSummary::from(entry.value()))
            .collect();

        // Sort by descending created_at so the newest tokens appear first.
        tokens.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Optional search filter (server-side, case-insensitive)
        if let Some(q) = &search {
            if !q.is_empty() {
                let q = q.to_lowercase();
                tokens.retain(|t| {
                    t.mint_address.to_lowercase().contains(&q)
                        || t.symbol.to_lowercase().contains(&q)
                        || t.name.to_lowercase().contains(&q)
                });
            }
        }

        let total = tokens.len();
        let limit = limit_q.max(1).min(5_000) as usize;
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

/// `GET /api/tokens/:mint/trades`
///
/// Returns all trades for a token in chronological order (cache first, else DB).
pub async fn get_trades(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let mint = path.into_inner();

    if let Some(entry) = state.token_cache.get(&mint) {
        return HttpResponse::Ok().json(entry.trades.clone());
    }

    let repo = TradeRepo::new(state.db.clone());
    match repo.find_by_mint_all(&mint).await {
        Ok(trades) => HttpResponse::Ok().json(trades),
        Err(e) => {
            tracing::error!("DB error fetching trades for {mint}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }))
        }
    }
}
