use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::info;

use crate::{
    models::ingest::{IngestKind, StrategyPing},
    state::token_cache::TokenCache,
    trader::PumpFunTrader,
};

use super::tpsl_sniper_1::{TpslRuntimeCache, TpslStrategyService};

/// Dispatches ingest pings to strategy implementations (token cache is source of truth).
pub struct StrategyRunner {
    token_cache: Arc<TokenCache>,
    tpsl: TpslStrategyService,
}

impl StrategyRunner {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        token_cache: Arc<TokenCache>,
        tpsl_cache: Arc<TpslRuntimeCache>,
    ) -> Self {
        let tpsl = TpslStrategyService::new(pool, trader, tpsl_cache);
        tpsl.spawn_background_tasks();
        Self { token_cache, tpsl }
    }

    pub async fn run(self, mut ping_rx: mpsc::Receiver<StrategyPing>) {
        info!("StrategyRunner: starting");

        while let Some(ping) = ping_rx.recv().await {
            match ping.kind {
                IngestKind::TokenCreated => {
                    self.tpsl
                        .on_token_created(&ping.mint, self.token_cache.as_ref())
                        .await;
                }
                IngestKind::Trade => {
                    self.tpsl
                        .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                        .await;
                }
                IngestKind::Migrated | IngestKind::CreatorActivity => {}
            }
        }

        info!("StrategyRunner: ping channel closed — stopping");
    }
}
