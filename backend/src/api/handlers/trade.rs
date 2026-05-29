use std::collections::HashMap;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::services::TradingService;
use crate::state::app_state::AppState;

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
