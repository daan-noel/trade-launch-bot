use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc};
use tracing::info;

use crate::{
    models::ingest::{IngestKind, SseEvent, StrategyPing},
    state::token_cache::TokenCache,
    trader::PumpFunTrader,
};

use super::tpsl_sniper_1::{Tpsl1RuntimeCache, Tpsl1StrategyService};
use super::tpsl_sniper_2::{Tpsl2RuntimeCache, Tpsl2StrategyService};

/// Dispatches ingest pings to strategy implementations (token cache is source of truth).
pub struct StrategyRunner {
    token_cache: Arc<TokenCache>,
    tpsl1: Tpsl1StrategyService,
    tpsl2: Tpsl2StrategyService,
}

impl StrategyRunner {
    pub fn new(
        pool: PgPool,
        trader: Arc<PumpFunTrader>,
        token_cache: Arc<TokenCache>,
        tpsl1_cache: Arc<Tpsl1RuntimeCache>,
        tpsl2_cache: Arc<Tpsl2RuntimeCache>,
        sse_tx: broadcast::Sender<SseEvent>,
    ) -> Self {
        let tpsl1 = Tpsl1StrategyService::new(pool.clone(), trader.clone(), tpsl1_cache, sse_tx.clone());
        tpsl1.spawn_background_tasks();
        let tpsl2 = Tpsl2StrategyService::new(pool, trader, tpsl2_cache, sse_tx);
        tpsl2.spawn_background_tasks();
        Self { token_cache, tpsl1, tpsl2 }
    }

    /// How often the wall-clock exit sweep runs. Time/stall stops are minute-scale,
    /// so a 1s cadence is plenty while staying cheap (it iterates only open
    /// positions and no-ops for rules without time-based exits).
    const TIME_EXIT_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

    pub async fn run(self, mut ping_rx: mpsc::Receiver<StrategyPing>) {
        info!("StrategyRunner: starting");

        // The sweep shares this single task with ping handling (via `select!`), so
        // a time-driven exit can never interleave with `on_trade_executed` on the
        // same position — the Holding→ExitPending transition stays serialized
        // without any DB-level locking.
        let mut sweep = tokio::time::interval(Self::TIME_EXIT_SWEEP_INTERVAL);
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                maybe_ping = ping_rx.recv() => {
                    let Some(ping) = maybe_ping else { break };
                    match ping.kind {
                        IngestKind::TokenCreated => {
                            self.tpsl1
                                .on_token_created(&ping.mint, self.token_cache.as_ref())
                                .await;
                            self.tpsl2
                                .on_token_created(&ping.mint, self.token_cache.as_ref())
                                .await;
                        }
                        IngestKind::Trade => {
                            self.tpsl1
                                .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                                .await;
                            self.tpsl2
                                .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                                .await;
                        }
                        IngestKind::Migrated | IngestKind::CreatorActivity => {}
                    }
                }
                _ = sweep.tick() => {
                    // Time-driven exits (TimeStop / Stall) that come due while a
                    // token is silent — no trade ping would otherwise fire them.
                    self.tpsl1
                        .sweep_time_exits(self.token_cache.as_ref())
                        .await;
                    self.tpsl2
                        .sweep_time_exits(self.token_cache.as_ref())
                        .await;
                }
            }
        }

        info!("StrategyRunner: ping channel closed — stopping");
    }
}
