use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::config::constants::TOKEN_PROGRAM_ID;
use crate::services::wallet_tokens;
use crate::state::app_state::AppState;

#[derive(Deserialize)]
pub struct BuyRequest {
    pub mint: String,
    pub sol_amount: f64,
    /// Optional: when omitted (manual buy by mint), the backend resolves it
    /// on-chain alongside the migration status.
    pub token_program_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SellRequest {
    pub mint: String,
    pub token_amount: u64,
    pub token_account: Option<String>,
}

/// POST /api/solana/wallet/buy
pub async fn manual_buy(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<BuyRequest>,
) -> impl Responder {
    // Resolve routing facts (creator, token program, migration status) from
    // chain — the source of truth, so a typed-in mint the cache has never seen,
    // a just-migrated token, or a Token-2022 mint all route correctly.
    let routing = match app_state.trader.resolve_buy_routing(&body.mint).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("manual_buy: resolve_buy_routing failed for {}: {e}", body.mint);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Could not resolve token: {e}") }));
        }
    };

    // Caller-supplied token program wins; otherwise use the on-chain owner.
    let token_program_id = body
        .token_program_id
        .clone()
        .unwrap_or(routing.token_program_id);
    let sol_amount = body.sol_amount;

    let buy_result = if routing.is_migrated {
        // Migrated → PumpSwap AMM (canonical pool derived, default slippage).
        // Mayhem tokens never migrate, so the AMM path needs no mayhem handling.
        app_state
            .trader
            .amm_buy(&body.mint, &token_program_id, sol_amount, None, None)
            .await
    } else {
        // Still on the bonding curve.
        app_state
            .trader
            .buy_token(&body.mint, &routing.creator, &token_program_id, sol_amount)
            .await
    };
    match buy_result {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_buy failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// POST /api/solana/wallet/sell
pub async fn manual_sell(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<SellRequest>,
) -> impl Responder {
    let token_account_override = body.token_account.as_deref();
    let sell_amount = (body.token_amount * 90 / 100) as u64; // Sell 99% to avoid dust issues

    // Routing facts from the in-memory cache (extract values, then drop the Ref
    // before any await): is_migrated picks bonding-curve vs PumpSwap AMM,
    // is_cashback gates the curve's cashback account, token_program_id builds
    // AMM accounts.
    let (is_migrated, is_cashback, base_tp) = match app_state.token_cache.get(&body.mint) {
        Some(e) => (
            e.is_migrated,
            e.token.is_cashback_enabled,
            e.token.token_program_id.clone(),
        ),
        None => (false, false, None),
    };
    let base_tp = base_tp.unwrap_or_else(|| TOKEN_PROGRAM_ID.to_string());

    let sell_result = if is_migrated {
        app_state
            .trader
            .amm_sell(&body.mint, sell_amount, &base_tp, None, token_account_override, None)
            .await
    } else {
        app_state
            .trader
            .sell_token(&body.mint, sell_amount, None, is_cashback, token_account_override)
            .await
    };
    match sell_result {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_sell failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// GET /api/solana/wallet/tokens
///
/// Returns all non-zero token accounts held by the trader's wallet,
/// enriched with symbol (from token cache) and current USD price (Jupiter).
pub async fn get_wallet_tokens(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    match wallet_tokens::list_enriched(app_state.get_ref()).await {
        Ok(enriched) => HttpResponse::Ok().json(enriched),
        Err(e) => {
            tracing::warn!("get_wallet_tokens failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// GET /api/solana/wallet/{wallet}/token/{mint}
///
/// Returns the on-chain token balance for the given wallet and mint,
/// queried directly from Solana — does not touch the local database.
pub async fn get_wallet_token_balance(
    app_state: web::Data<Arc<AppState>>,
    path: web::Path<(String, String)>,
) -> impl Responder {
    let (wallet, mint) = path.into_inner();
    match app_state.trader.get_token_balance(&wallet, &mint).await {
        Ok(balance) => HttpResponse::Ok().json(balance),
        Err(e) => {
            tracing::warn!("get_wallet_token_balance failed wallet={wallet} mint={mint}: {e}");
            HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
