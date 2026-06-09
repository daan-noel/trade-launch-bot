use std::collections::HashMap;

use crate::services::http;

#[derive(Debug, Default, Clone)]
pub struct JupiterPriceEntry {
    pub price_usd: Option<f64>,
    pub liquidity: Option<f64>,
    pub price_change_24h: Option<f64>,
    pub token_created_at: Option<String>,
}

pub async fn fetch_prices(mints: &[String]) -> anyhow::Result<HashMap<String, JupiterPriceEntry>> {
    if mints.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = mints.join(",");
    let base = &crate::config::settings::get().jupiter_price_api_url;
    let url = format!("{base}?ids={ids}");
    let resp: serde_json::Value = http::client().get(&url).send().await?.json().await?;

    let mut result = HashMap::new();
    if let Some(data) = resp.as_object() {
        for (mint, entry) in data {
            result.insert(
                mint.clone(),
                JupiterPriceEntry {
                    price_usd: entry["usdPrice"].as_f64(),
                    liquidity: entry["liquidity"].as_f64(),
                    price_change_24h: entry["priceChange24h"].as_f64(),
                    token_created_at: entry["createdAt"].as_str().map(str::to_owned),
                },
            );
        }
    }
    Ok(result)
}
