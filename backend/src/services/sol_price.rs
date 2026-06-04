use std::sync::Arc;

use tokio::sync::watch;
use tracing::{info, warn};

use crate::state::app_state::AppState;

use super::clients::coingecko;

const POLL_INTERVAL_SECS: u64 = 60;

pub async fn fetch_latest_sol_price() -> anyhow::Result<f64> {
    coingecko::fetch_sol_usd().await
}

/// Fetches SOL/USD from CoinGecko and updates `AppState`; falls back to cached value on error.
pub async fn refresh(state: &AppState) -> Option<f64> {
    match fetch_latest_sol_price().await {
        Ok(price) => {
            state.set_sol_price(Some(price));
            Some(price)
        }
        Err(err) => {
            warn!("Failed to refresh SOL price: {err}");
            state.latest_sol_price()
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
