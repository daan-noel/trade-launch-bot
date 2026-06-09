use reqwest::header::{ACCEPT, USER_AGENT};

use crate::services::http;

pub async fn fetch_sol_usd() -> anyhow::Result<f64> {
    let base = &crate::config::settings::get().coingecko_price_url;
    let url = format!("{base}?ids=solana&vs_currencies=usd");
    let client = http::client();
    let resp = client
        .get(&url)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, http::USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let body = resp.text().await?;
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("Failed to parse CoinGecko response: {e}: {body}"))?;

    json.get("solana")
        .and_then(|sol| sol.get("usd"))
        .and_then(|usd| usd.as_f64())
        .ok_or_else(|| anyhow::anyhow!("Unexpected CoinGecko response shape: {body}"))
}
