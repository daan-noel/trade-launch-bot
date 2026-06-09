use serde::Serialize;
use tracing::warn;

use crate::services::clients::jupiter;
use crate::state::app_state::AppState;
use crate::trader::WalletHolding;

#[derive(Debug, Serialize)]
pub struct EnrichedWalletHolding {
    pub mint: String,
    pub amount: u64,
    pub ui_amount: f64,
    pub decimals: u8,
    pub token_account: String,
    pub token_program_id: String,
    pub symbol: Option<String>,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    pub liquidity: Option<f64>,
    pub price_change_24h: Option<f64>,
    pub token_created_at: Option<String>,
    /// Whether this token has migrated from the bonding curve to the AMM.
    pub is_migrated: bool,
    /// Whether cashback was enabled at token creation (create_v2 only).
    pub is_cashback_enabled: bool,
}

pub async fn list_enriched(state: &AppState) -> anyhow::Result<Vec<EnrichedWalletHolding>> {
    let holdings = state.trader.get_all_token_accounts().await?;
    Ok(enrich_holdings(state, holdings).await)
}

async fn enrich_holdings(
    state: &AppState,
    holdings: Vec<WalletHolding>,
) -> Vec<EnrichedWalletHolding> {
    let mints: Vec<String> = holdings.iter().map(|h| h.mint.clone()).collect();

    let jupiter = match jupiter::fetch_prices(&mints).await {
        Ok(prices) => prices,
        Err(e) => {
            warn!("Jupiter price fetch failed: {e}");
            Default::default()
        }
    };

    holdings
        .into_iter()
        .map(|h| {
            let entry = jupiter.get(&h.mint);
            let price_usd = entry.and_then(|e| e.price_usd);
            let value_usd = price_usd.map(|p| p * h.ui_amount);
            let cached = state.token_cache.get(&h.mint);
            EnrichedWalletHolding {
                symbol: cached.as_ref().map(|s| s.token.symbol.clone()),
                is_migrated: cached.as_ref().map(|s| s.is_migrated).unwrap_or(false),
                is_cashback_enabled: cached
                    .as_ref()
                    .map(|s| s.token.is_cashback_enabled)
                    .unwrap_or(false),
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
        .collect()
}
