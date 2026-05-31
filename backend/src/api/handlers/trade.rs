use std::collections::HashMap;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::services::TradingService;
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
}

/// POST /api/solana/wallet/buy
pub async fn manual_buy(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<BuyRequest>,
) -> impl Responder {
    let rpc_url = app_state.trader.rpc_url().to_string();
    let creator = match fetch_creator_from_helius(&rpc_url, &body.mint).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("manual_buy: getAsset failed for {}: {e}", body.mint);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Could not resolve creator: {e}") }));
        }
    };

    let trading = TradingService::new(app_state.trader.clone());
    match trading.buy_token(&body.mint, &creator, &body.token_program_id, body.sol_amount).await {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_buy failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// Calls Helius DAS `getAsset` and returns the first authority address as the creator.
/// Falls back to the first verified creator if no authority is found.
async fn fetch_creator_from_helius(rpc_url: &str, mint: &str) -> anyhow::Result<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "1",
        "method": "getAsset",
        "params": { "id": mint }
    });

    let resp: serde_json::Value = reqwest::Client::new()
        .post(rpc_url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let result = &resp["result"];

    // Prefer authorities — most reliable for pump.fun tokens
    if let Some(auth) = result["authorities"].as_array().and_then(|a| a.first()) {
        if let Some(addr) = auth["address"].as_str() {
            return Ok(addr.to_owned());
        }
    }

    // Fall back to first verified creator
    if let Some(creators) = result["creators"].as_array() {
        for creator in creators {
            if creator["verified"].as_bool().unwrap_or(false) {
                if let Some(addr) = creator["address"].as_str() {
                    return Ok(addr.to_owned());
                }
            }
        }
        // Last resort: any creator
        if let Some(addr) = creators.first().and_then(|c| c["address"].as_str()) {
            return Ok(addr.to_owned());
        }
    }

    anyhow::bail!("no authority or creator found in getAsset response")
}

/// POST /api/solana/wallet/sell
pub async fn manual_sell(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<SellRequest>,
) -> impl Responder {
    let trading = TradingService::new(app_state.trader.clone());
    match trading.sell_token(&body.mint, body.token_amount, None, false).await {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_sell failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Debug, Default)]
struct JupiterEntry {
    price_usd: Option<f64>,
    liquidity: Option<f64>,
    price_change_24h: Option<f64>,
    token_created_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct EnrichedWalletHolding {
    mint: String,
    amount: u64,
    ui_amount: f64,
    decimals: u8,
    token_account: String,
    token_program_id: String,
    symbol: Option<String>,
    price_usd: Option<f64>,
    value_usd: Option<f64>,
    liquidity: Option<f64>,
    price_change_24h: Option<f64>,
    token_created_at: Option<String>,
}

/// GET /api/solana/wallet/tokens
///
/// Returns all non-zero token accounts held by the trader's wallet,
/// enriched with symbol (from token cache) and current USD price (Jupiter).
pub async fn get_wallet_tokens(
    app_state: web::Data<Arc<AppState>>,
) -> impl Responder {
    let trading = TradingService::new(app_state.trader.clone());
    let holdings = match trading.get_all_token_accounts().await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("get_wallet_tokens failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }));
        }
    };

    let mints: Vec<String> = holdings.iter().map(|h| h.mint.clone()).collect();

    let symbols: HashMap<String, String> = mints
        .iter()
        .filter_map(|mint| {
            app_state
                .token_cache
                .get(mint)
                .map(|s| (mint.clone(), s.token.symbol.clone()))
        })
        .collect();

    let jupiter = fetch_jupiter_data(&mints).await.unwrap_or_default();

    let enriched: Vec<EnrichedWalletHolding> = holdings
        .into_iter()
        .map(|h| {
            let entry = jupiter.get(&h.mint);
            let price_usd = entry.and_then(|e| e.price_usd);
            let value_usd = price_usd.map(|p| p * h.ui_amount);
            EnrichedWalletHolding {
                symbol: symbols.get(&h.mint).cloned(),
                price_usd,
                value_usd,
                liquidity: entry.and_then(|e| e.liquidity),
                price_change_24h: entry.and_then(|e| e.price_change_24h),
                token_created_at: entry.and_then(|e| e.token_created_at.clone()),
                mint: h.mint,
                amount: h.amount,
                ui_amount: h.ui_amount,
                decimals: h.decimals,
                token_account: h.token_account,
                token_program_id: h.token_program_id,
            }
        })
        .collect();

    HttpResponse::Ok().json(enriched)
}

async fn fetch_jupiter_data(mints: &[String]) -> anyhow::Result<HashMap<String, JupiterEntry>> {
    if mints.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = mints.join(",");
    let url = format!("https://api.jup.ag/price/v3?ids={ids}");
    let resp: serde_json::Value = reqwest::Client::new()
        .get(&url)
        .send()
        .await?
        .json()
        .await?;

    let mut result = HashMap::new();
    if let Some(data) = resp.as_object() {
        for (mint, entry) in data {
            result.insert(mint.clone(), JupiterEntry {
                price_usd: entry["usdPrice"].as_f64(),
                liquidity: entry["liquidity"].as_f64(),
                price_change_24h: entry["priceChange24h"].as_f64(),
                token_created_at: entry["createdAt"].as_str().map(str::to_owned),
            });
        }
    }
    Ok(result)
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
    let trading = TradingService::new(app_state.trader.clone());

    match trading.get_token_balance(&wallet, &mint).await {
        Ok(balance) => HttpResponse::Ok().json(balance),
        Err(e) => {
            tracing::warn!("get_wallet_token_balance failed wallet={wallet} mint={mint}: {e}");
            HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
