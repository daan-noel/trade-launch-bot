use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc};
use tracing::info;

use crate::{
    models::ingest::{IngestKind, SseEvent, StrategyPing},
    state::token_cache::TokenCache,
    trader::PumpFunTrader,
};

use super::tpsl_sniper_1::{TpslRuntimeCache, TpslStrategyService};
use super::tpsl_sniper_2::{Tpsl2RuntimeCache, Tpsl2StrategyService};

/// Dispatches ingest pings to strategy implementations (token cache is source of truth).
pub struct StrategyRunner {
    token_cache: Arc<TokenCache>,
    tpsl: TpslStrategyService,
    tpsl2: Tpsl2StrategyService,
}

impl StrategyRunner {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        token_cache: Arc<TokenCache>,
        tpsl_cache: Arc<TpslRuntimeCache>,
        tpsl2_cache: Arc<Tpsl2RuntimeCache>,
        sse_tx: broadcast::Sender<SseEvent>,
    ) -> Self {
        let tpsl = TpslStrategyService::new(pool.clone(), trader.clone(), tpsl_cache, sse_tx.clone());
        tpsl.spawn_background_tasks();
        let tpsl2 = Tpsl2StrategyService::new(pool, trader, tpsl2_cache, sse_tx);
        tpsl2.spawn_background_tasks();
        Self { token_cache, tpsl, tpsl2 }
    }

    pub async fn run(self, mut ping_rx: mpsc::Receiver<StrategyPing>) {
        info!("StrategyRunner: starting");

        while let Some(ping) = ping_rx.recv().await {
            match ping.kind {
                IngestKind::TokenCreated => {
                    self.tpsl
                        .on_token_created(&ping.mint, self.token_cache.as_ref())
                        .await;
                    self.tpsl2
                        .on_token_created(&ping.mint, self.token_cache.as_ref())
                        .await;
                }
                IngestKind::Trade => {
                    self.tpsl
                        .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                        .await;
                    self.tpsl2
                        .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                        .await;
                }
                IngestKind::Migrated | IngestKind::CreatorActivity => {}
            }
        }

        info!("StrategyRunner: ping channel closed — stopping");
    }
}
