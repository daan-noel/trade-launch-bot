use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{info, warn};

use crate::state::app_state::AppState;

use super::clients::coingecko;

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

/// Background task that polls CoinGecko on `interval` and updates the SOL/USD watch channel.
pub async fn run_poller(sol_price_tx: Arc<watch::Sender<Option<f64>>>, interval: Duration) {
    info!("SOL price poller: starting (every {}s)", interval.as_secs());

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

        tokio::time::sleep(interval).await;
    }
}
