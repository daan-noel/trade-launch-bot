use std::time::Duration;

use reqwest::header::{ACCEPT, RETRY_AFTER, USER_AGENT};
use reqwest::StatusCode;
use serde::Deserialize;
use tokio::time::sleep;
use tracing::warn;

use crate::services::http;

const SOL_USD_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";

/// Bounded retry on transient failures (network error, 5xx, 429). Keyless
/// CoinGecko rate-limits aggressively, so honor `Retry-After` on a 429 and back
/// off exponentially otherwise — a flat re-poll would just keep tripping the limit.
const MAX_ATTEMPTS: usize = 3;
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

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
    let mut backoff = INITIAL_BACKOFF;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..MAX_ATTEMPTS {
        match client
            .get(SOL_USD_URL)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, http::USER_AGENT)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                // Rate-limited: honor Retry-After (or back off) and retry.
                if status == StatusCode::TOO_MANY_REQUESTS {
                    let wait = retry_after(&resp).unwrap_or(backoff);
                    warn!("CoinGecko 429 (attempt {attempt}); backing off {wait:?}");
                    last_err = Some(anyhow::anyhow!("CoinGecko rate-limited (429)"));
                    sleep(wait).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
                // Transient server error: back off and retry.
                if status.is_server_error() {
                    warn!("CoinGecko {status} (attempt {attempt}); backing off {backoff:?}");
                    last_err = Some(anyhow::anyhow!("CoinGecko server error {status}"));
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
                // Other non-success (4xx): not retryable.
                let resp = resp.error_for_status()?;
                let price = resp
                    .json::<SimplePrice>()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to parse CoinGecko response: {e}"))?;
                return Ok(price.solana.usd);
            }
            Err(e) => {
                // Network/timeout error: back off and retry.
                warn!("CoinGecko request failed (attempt {attempt}): {e}");
                last_err = Some(anyhow::anyhow!("CoinGecko request: {e}"));
                sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("CoinGecko: exhausted retries")))
}

/// Parse a `Retry-After: <seconds>` header into a duration (CoinGecko uses the
/// delta-seconds form). Ignores the HTTP-date form (rare here).
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}
