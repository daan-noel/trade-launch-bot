use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::info;

use crate::state::token_cache::TokenCache;
use trading_core::models::ingest::{IngestKind, StrategyPing};

use super::service::StrategyService;

/// Dispatches ingest pings to the unified, strategy-agnostic [`StrategyService`]
/// (which routes each rule by `strategy_id`). The token cache is the source of
/// truth for the decision path.
pub struct StrategyRunner {
    token_cache: Arc<TokenCache>,
    service: StrategyService,
}

impl StrategyRunner {
    /// Build the runner from an already-constructed service (its background reapers
    /// are spawned by the caller — `main` — so `DeployState` can share the same
    /// service handle for the rule-CRUD lifecycle).
    pub fn new(service: StrategyService, token_cache: Arc<TokenCache>) -> Self {
        Self { token_cache, service }
    }

    /// How often the wall-clock exit sweep runs. Time/stall stops are minute-scale,
    /// so a 1s cadence is plenty while staying cheap (it iterates only the time-exit
    /// holdings index and no-ops for rules without time-based exits).
    const TIME_EXIT_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

    pub async fn run(self, mut ping_rx: mpsc::Receiver<StrategyPing>) {
        info!("StrategyRunner: starting");

        // The sweep shares this single task with ping handling (via `select!`), so a
        // time-driven exit can never interleave with `on_trade_executed` on the same
        // position — the Holding→ExitPending transition stays serialized without any
        // DB-level locking.
        let mut sweep = tokio::time::interval(Self::TIME_EXIT_SWEEP_INTERVAL);
        sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                maybe_ping = ping_rx.recv() => {
                    let Some(ping) = maybe_ping else { break };
                    match ping.kind {
                        IngestKind::TokenCreated => {
                            self.service
                                .on_token_created(&ping.mint, self.token_cache.as_ref())
                                .await;
                        }
                        IngestKind::Trade => {
                            self.service
                                .on_trade_executed(&ping.mint, self.token_cache.as_ref())
                                .await;
                        }
                        // Not entry/exit triggers: a migration's routing change is
                        // re-read from the WS cache on every sell attempt inside the
                        // real sell loop, and creator activity has no strategy action.
                        IngestKind::Migrated | IngestKind::CreatorActivity => {}
                    }
                }
                _ = sweep.tick() => {
                    // Time-driven exits (TimeStop / Stall) that come due while a token
                    // is silent — no trade ping would otherwise fire them.
                    self.service.sweep_time_exits(self.token_cache.as_ref()).await;
                    self.service
                        .sweep_first_slot_pending(self.token_cache.as_ref())
                        .await;
                }
            }
        }

        info!("StrategyRunner: ping channel closed — stopping");
    }
}
