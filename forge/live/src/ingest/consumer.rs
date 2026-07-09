//! The ingest → DB pipeline (LIVE box). Builds the borrowed transport, then
//! consumes `IngestEvent`s and projects them onto `raw_txs` / `trades` / `tokens`
//! through the pump.fun/SOL adapter.
//!
//! Batching: trades + raw_txs accumulate in buffers flushed on size or a short
//! interval (one `UNNEST` round-trip per flush). Wallet addresses are interned
//! through an in-memory cache so a repeat wallet costs no DB round-trip. Raw txs
//! are persisted only for txs that produced a semantic event (the trailing
//! `RawTx` event uses a per-tx flag), so `raw_txs` stays the source-of-truth for
//! exactly what `trades` projects.
//!
//! This is the scaffold pipeline (single task, inline batched writer). A fully
//! channel-decoupled `DbWriter` is a later refinement; correctness + idempotent
//! replay (PK `ON CONFLICT DO NOTHING`) hold today.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ingest_laserstream::{Ingest, IngestConfig, IngestEvent, IngestHandle, Protocol};
use sqlx::PgPool;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use platform_core::models::RawTx;
use platform_core::models::NewTrade;
use platform_core::storage::repositories::dimensions::NewMarket;
use platform_core::storage::repositories::{MarketRepo, RawTxRepo, TokenRepo, TradeRepo, WalletDictRepo};
use platform_core::venue::{LaunchpadAdapter, MarketKind};

use super::map;
use super::pumpfun::{PumpFunAdapter, CURVE_PROGRAM_ID};

/// Flush when a buffer reaches this many rows (or on the interval, whichever first).
const FLUSH_EVERY: usize = 256;
/// Max time rows sit buffered before a flush.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Build the transport and spawn the ingest → DB pipeline task. Streams live
/// immediately (`start(true)`). Returns the consumer's join handle plus the
/// control handle (the caller owns it for the process lifetime — e.g. to
/// expose a runtime pause/resume toggle over HTTP).
pub async fn spawn_ingest(
    pool: PgPool,
    endpoint: String,
    api_key: String,
) -> anyhow::Result<(JoinHandle<()>, Arc<IngestHandle>)> {
    let adapter = PumpFunAdapter::resolve(&pool).await?;
    let (event_rx, handle) = Ingest::builder()
        .endpoint(endpoint)
        .api_key(api_key)
        .protocol(Protocol::pump_fun())
        .config(IngestConfig::default())
        .build()?
        .start(true);
    let handle = Arc::new(handle);
    info!("ingest transport started (pump.fun/SOL adapter)");
    let join = tokio::spawn(run_consumer(pool, adapter, event_rx));
    Ok((join, handle))
}

async fn run_consumer(
    pool: PgPool,
    adapter: PumpFunAdapter,
    mut event_rx: Receiver<IngestEvent>,
) {
    let mut trades: Vec<NewTrade> = Vec::new();
    let mut raws: Vec<RawTx> = Vec::new();
    let mut wallet_cache: HashMap<String, i32> = HashMap::new();
    let mut tracked_in_tx = false;
    let mut tick = tokio::time::interval(FLUSH_INTERVAL);

    loop {
        tokio::select! {
            maybe = event_rx.recv() => {
                let Some(ev) = maybe else {
                    warn!("ingest event channel closed — transport ended");
                    break;
                };
                match ev {
                    IngestEvent::Trade(t) => {
                        tracked_in_tx = true;
                        match wallet_id(&pool, &mut wallet_cache, &t.wallet).await {
                            Ok(wid) => match map::trade_to_row(&adapter, wid, &t) {
                                Ok(row) => trades.push(row),
                                Err(e) => warn!(?e, mint = %t.mint, "skip trade (sig decode)"),
                            },
                            Err(e) => warn!(?e, "wallet intern failed"),
                        }
                        if trades.len() >= FLUSH_EVERY {
                            flush(&pool, &mut trades, &mut raws).await;
                        }
                    }
                    IngestEvent::TokenCreated(tc) => {
                        tracked_in_tx = true;
                        let token = map::token_created_to_row(&adapter, &tc);
                        if let Err(e) = TokenRepo::insert(&pool, &token).await {
                            warn!(?e, mint = %tc.mint, "token insert failed");
                        }
                        let market = NewMarket {
                            mint_address: tc.mint.clone(),
                            launchpad_id: adapter.launchpad_id(),
                            market_kind: MarketKind::BondingCurve.as_str().to_string(),
                            program_id: CURVE_PROGRAM_ID.to_string(),
                            quote_asset_id: adapter.quote_asset_id(MarketKind::BondingCurve),
                            pool_address: tc.bonding_curve.clone(),
                            created_slot: Some(tc.slot as i64),
                        };
                        if let Err(e) = MarketRepo::upsert(&pool, &market).await {
                            warn!(?e, mint = %tc.mint, "market upsert failed");
                        }
                    }
                    IngestEvent::RawTx(r) => {
                        if tracked_in_tx {
                            raws.push(map::raw_tx_to_row(&r));
                        }
                        tracked_in_tx = false;
                        if raws.len() >= FLUSH_EVERY {
                            flush(&pool, &mut trades, &mut raws).await;
                        }
                    }
                    // TokenMigrated / Liquidity / CreatorActivity: not projected yet.
                    _ => {}
                }
            }
            _ = tick.tick() => {
                flush(&pool, &mut trades, &mut raws).await;
            }
        }
    }
    // Final drain.
    flush(&pool, &mut trades, &mut raws).await;
}

/// Intern a wallet address → id, memoized. On a cache hit no DB round-trip occurs.
async fn wallet_id(
    pool: &PgPool,
    cache: &mut HashMap<String, i32>,
    address: &str,
) -> anyhow::Result<i32> {
    if let Some(id) = cache.get(address) {
        return Ok(*id);
    }
    let id = WalletDictRepo::intern(pool, address).await?;
    cache.insert(address.to_string(), id);
    Ok(id)
}

/// Flush both buffers (idempotent inserts). Errors are logged, not propagated —
/// a transient DB error must not tear down the ingest task.
async fn flush(pool: &PgPool, trades: &mut Vec<NewTrade>, raws: &mut Vec<RawTx>) {
    if !trades.is_empty() {
        match TradeRepo::insert_batch(pool, trades).await {
            Ok(n) => tracing::debug!(rows = n, "flushed trades"),
            Err(e) => warn!(?e, count = trades.len(), "trade flush failed"),
        }
        trades.clear();
    }
    if !raws.is_empty() {
        match RawTxRepo::insert_batch(pool, raws).await {
            Ok(n) => tracing::debug!(rows = n, "flushed raw_txs"),
            Err(e) => warn!(?e, count = raws.len(), "raw_tx flush failed"),
        }
        raws.clear();
    }
}
