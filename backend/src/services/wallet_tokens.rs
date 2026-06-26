use serde::Serialize;
use tracing::warn;

use crate::services::clients::jupiter;
use crate::state::deploy_state::DeployState;
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

pub async fn list_enriched(state: &DeployState) -> anyhow::Result<Vec<EnrichedWalletHolding>> {
    let holdings = state.trader.get_all_token_accounts().await?;
    Ok(enrich_holdings(state, holdings).await)
}

/// Enrich a single mint's holding for the trader's wallet, or `None` if the
/// wallet holds none of it. A cheap one-mint read (single RPC + one Jupiter
/// price) used to confirm a balance change after a manual trade without
/// re-scanning and re-pricing the whole wallet.
pub async fn enrich_one(
    state: &DeployState,
    mint: &str,
) -> anyhow::Result<Option<EnrichedWalletHolding>> {
    let Some(holding) = state.trader.get_token_account_for_mint(mint).await? else {
        return Ok(None);
    };
    Ok(enrich_holdings(state, vec![holding]).await.into_iter().next())
}

async fn enrich_holdings(
    state: &DeployState,
    holdings: Vec<WalletHolding>,
) -> Vec<EnrichedWalletHolding> {
    let mints: Vec<String> = holdings.iter().map(|h| h.mint.clone()).collect();

    // The cache is authoritative for tracked tokens — the live stream flips
    // `is_migrated` on the migration event, and `is_cashback_enabled` is set at
    // create-decode time. Only mints the cache has never seen (e.g. one bought
    // manually and never ingested) need the on-chain fallback, so resolve just
    // those; with none, `resolve_curve_facts_batch` skips the RPC.
    let uncached: Vec<String> = mints
        .iter()
        .filter(|m| !state.token_cache.contains_key(m.as_str()))
        .cloned()
        .collect();

    // Prices (Jupiter) and the on-chain bonding-curve fallback (migration +
    // cashback, both read from the same account) are independent network reads
    // — fire them together.
    let (jupiter, chain_facts) = tokio::join!(
        jupiter::fetch_prices(&mints),
        state.trader.resolve_curve_facts_batch(&uncached),
    );
    let jupiter = match jupiter {
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
                // Cache first (live stream is authoritative for tracked tokens);
                // fall back to the on-chain read only for mints the cache never
                // saw, so a manually-bought migrated token still shows as AMM.
                is_migrated: cached
                    .as_ref()
                    .map(|s| s.is_migrated)
                    .or_else(|| chain_facts.get(&h.mint).map(|f| f.is_migrated))
                    .unwrap_or(false),
                // Same fallback shape as migration: the create_v2 cashback flag
                // persists on the bonding-curve account (offset 82), so a mint
                // the cache never saw still resolves from chain instead of
                // silently defaulting to "no cashback".
                is_cashback_enabled: cached
                    .as_ref()
                    .map(|s| s.token.is_cashback_enabled)
                    .or_else(|| chain_facts.get(&h.mint).map(|f| f.cashback_enabled))
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
