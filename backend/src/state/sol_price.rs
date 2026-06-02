use reqwest::header::{ACCEPT, USER_AGENT};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

const POLL_INTERVAL_SECS: u64 = 60;
const COINGECKO_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";
const USER_AGENT_STR: &str =
    "Mozilla/5.0 (compatible; MemeTrading/1.0; +https://github.com/your-org/meme-trading)";

/// Background task that polls CoinGecko every 60 s and updates the SOL/USD watch channel.
pub async fn run_poller(sol_price_tx: Arc<watch::Sender<Option<f64>>>) {
    info!("SOL price poller: starting (every {POLL_INTERVAL_SECS}s)");

    loop {
        match fetch_latest_sol_price().await {
            Ok(price) => {
                info!("SOL/USD price: ${price:.2}");
                let _ = sol_price_tx.send(Some(price));
            }
            Err(e) => {
                warn!("SOL price poller: fetch failed: {e}");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

pub async fn fetch_latest_sol_price() -> anyhow::Result<f64> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(USER_AGENT_STR)
        .build()?;
    fetch_sol_price(&client).await
}

async fn fetch_sol_price(client: &reqwest::Client) -> anyhow::Result<f64> {
    let resp = client
        .get(COINGECKO_URL)
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, USER_AGENT_STR)
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
