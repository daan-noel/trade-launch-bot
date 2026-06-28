use std::sync::Arc;

use tokio::sync::watch;
use tracing::{info, warn};

use super::clients::{coingecko, jupiter};

const POLL_INTERVAL_SECS: u64 = 60;

/// Fetch SOL/USD, preferring CoinGecko and falling back to Jupiter so a single
/// source being down or rate-limited doesn't stall the whole price feed.
pub async fn fetch_latest_sol_price() -> anyhow::Result<f64> {
    match coingecko::fetch_sol_usd().await {
        Ok(price) => Ok(price),
        Err(cg_err) => {
            warn!("SOL price: CoinGecko failed ({cg_err}); trying Jupiter fallback");
            jupiter::fetch_sol_usd().await.map_err(|jup_err| {
                anyhow::anyhow!("both SOL price sources failed: coingecko={cg_err}; jupiter={jup_err}")
            })
        }
    }
}

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
