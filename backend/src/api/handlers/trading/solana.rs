use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::services::wallet_tokens;
use crate::state::app_state::AppState;


#[derive(Deserialize)]
pub struct BuyRequest {
    pub mint: String,
    pub sol_amount: f64,
    pub token_program_id: String,
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
    let creator = match app_state.trader.get_creator_from_mint_pda(&body.mint).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("manual_buy: get_creator_from_mint_pda failed for {}: {e}", body.mint);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Could not resolve creator: {e}") }));
        }
    };

    let mint = body.mint.clone();
    let creator = creator.clone();
    let token_program_id = body.token_program_id.clone();
    let sol_amount = body.sol_amount;
    let buy_result = app_state
        .trader
        .buy_token(&mint, &creator, &token_program_id, sol_amount)
        .await;
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
    match app_state
        .trader
        .sell_token(&body.mint, sell_amount, None, false, token_account_override)
        .await
    {
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
