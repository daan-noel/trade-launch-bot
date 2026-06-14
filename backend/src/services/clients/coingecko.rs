use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use crate::services::http;

const SOL_USD_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";

/// `{"solana":{"usd":123.45}}`
#[derive(Deserialize)]
struct SimplePrice {
    solana: SolEntry,
}

#[derive(Deserialize)]
struct SolEntry {
    usd: f64,
}

pub async fn fetch_sol_usd() -> anyhow::Result<f64> {
    let client = http::client();
    let resp = client
        .get(SOL_USD_URL)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, http::USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let price = resp
        .json::<SimplePrice>()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse CoinGecko response: {e}"))?;

    Ok(price.solana.usd)
}
