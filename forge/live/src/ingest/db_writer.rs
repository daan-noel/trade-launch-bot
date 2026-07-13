//! Decoupled batched DB writer for the ingest pipeline (LIVE box).
//!
//! The hot-path [`consumer`](super::consumer) does **no** DB I/O: it maps nothing
//! and blocks on nothing but the bounded channel into this task. This `DbWriter`
//! owns the pool, the venue adapter and the wallet-intern cache, accumulates
//! [`DbWriteOp`]s into batches, and flushes them on a size or time trigger — one
//! `UNNEST` round-trip per buffer. A DB stall therefore backs up the channel and
//! (eventually) backpressures the transport, instead of stalling the recv loop
//! that drains the gRPC feed. The [`watchdog`](super::watchdog) force-exits the
//! process if this task stops committing while work is queued.
//!
//! All ops are **durable** (real, non-recomputable trades/tokens/raws), so there
//! is no metric-shedding tier as in hunter — the bounded channel's awaiting
//! backpressure is the (correct) overflow behavior for durable writes.

use std::collections::HashMap;
use std::time::Duration;

use ingest_laserstream::event::{RawTx as IlRawTx, TokenCreated as IlTokenCreated, Trade as IlTrade};
use sqlx::PgPool;
use tokio::sync::mpsc::Receiver;
use tracing::warn;

use platform_core::models::{NewTrade, RawTx};
use platform_core::storage::repositories::dimensions::NewMarket;
use platform_core::storage::repositories::{MarketRepo, RawTxRepo, TokenRepo, TradeRepo, WalletDictRepo};
use platform_core::venue::{LaunchpadAdapter, MarketKind};

use super::map;
use super::pumpfun::{PumpFunAdapter, CURVE_PROGRAM_ID};
use super::watchdog::DbHeartbeat;

/// Bounded channel depth from consumer → writer. Full ⇒ the consumer's
/// `send().await` blocks, backpressuring the transport (durable writes are never
/// silently dropped). Sized generously for burst tolerance on the small box.
pub const CHANNEL_CAPACITY: usize = 16_384;

/// Flush when the batch reaches this many ops (or on the interval, whichever first).
const FLUSH_EVERY: usize = 256;
/// Max time ops sit buffered before a flush.
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// One unit of durable work handed to the writer. The consumer forwards raw feed
/// events verbatim — all mapping (which needs the adapter / wallet cache) happens
/// here, off the recv loop.
pub enum DbWriteOp {
    Trade(Box<IlTrade>),
    TokenCreated(Box<IlTokenCreated>),
    Raw(Box<IlRawTx>),
}

/// Owns the write side of the pipeline. Spawned once by `spawn_ingest`.
pub struct DbWriter {
    pool: PgPool,
    adapter: PumpFunAdapter,
    /// address → wallet_dict id, memoized across flushes (a repeat wallet costs no
    /// round-trip). Lives on the writer task, never the hot path.
    wallet_cache: HashMap<String, i32>,
    heartbeat: DbHeartbeat,
}

impl DbWriter {
    pub fn new(pool: PgPool, adapter: PumpFunAdapter, heartbeat: DbHeartbeat) -> Self {
        Self {
            pool,
            adapter,
            wallet_cache: HashMap::new(),
            heartbeat,
        }
    }

    pub async fn run(mut self, mut rx: Receiver<DbWriteOp>) {
        let mut batch: Vec<DbWriteOp> = Vec::with_capacity(FLUSH_EVERY);
        let mut tick = tokio::time::interval(FLUSH_INTERVAL);
        loop {
            tokio::select! {
                op = rx.recv() => {
                    match op {
                        Some(op) => {
                            batch.push(op);
                            if batch.len() >= FLUSH_EVERY {
                                self.flush(&mut batch).await;
                            }
                        }
                        None => {
                            // Consumer dropped its sender (transport ended) — final drain, exit.
                            self.flush(&mut batch).await;
                            warn!("ingest DbWriter: channel closed — writer exiting");
                            return;
                        }
                    }
                }
                _ = tick.tick() => {
                    if !batch.is_empty() {
                        self.flush(&mut batch).await;
                    }
                }
            }
        }
    }

    async fn flush(&mut self, batch: &mut Vec<DbWriteOp>) {
        if batch.is_empty() {
            return;
        }
        let mut trades: Vec<IlTrade> = Vec::new();
        let mut tokens: Vec<IlTokenCreated> = Vec::new();
        let mut raws: Vec<RawTx> = Vec::new();
        for op in batch.drain(..) {
            match op {
                DbWriteOp::Trade(t) => trades.push(*t),
                DbWriteOp::TokenCreated(tc) => tokens.push(*tc),
                DbWriteOp::Raw(r) => raws.push(map::raw_tx_to_row(&r)),
            }
        }

        // Tokens + their bonding-curve market first (rare relative to trades;
        // per-row is fine and keeps the existing insert/upsert semantics).
        for tc in &tokens {
            let token = map::token_created_to_row(&self.adapter, tc);
            if let Err(e) = TokenRepo::insert(&self.pool, &token).await {
                warn!(?e, mint = %tc.mint, "token insert failed");
            }
            let market = NewMarket {
                mint_address: tc.mint.clone(),
                launchpad_id: self.adapter.launchpad_id(),
                market_kind: MarketKind::BondingCurve.as_str().to_string(),
                program_id: CURVE_PROGRAM_ID.to_string(),
                quote_asset_id: self.adapter.quote_asset_id(MarketKind::BondingCurve),
                pool_address: tc.bonding_curve.clone(),
                created_slot: Some(tc.slot as i64),
            };
            if let Err(e) = MarketRepo::upsert(&self.pool, &market).await {
                warn!(?e, mint = %tc.mint, "market upsert failed");
            }
        }

        // Trades: intern wallets (cached), map, then one batch insert.
        if !trades.is_empty() {
            let mut rows: Vec<NewTrade> = Vec::with_capacity(trades.len());
            for t in &trades {
                let wid = match self.wallet_ref(&t.wallet).await {
                    Ok(id) => id,
                    Err(e) => {
                        warn!(?e, "wallet intern failed — skip trade");
                        continue;
                    }
                };
                match map::trade_to_row(&self.adapter, wid, t) {
                    Ok(row) => rows.push(row),
                    Err(e) => warn!(?e, mint = %t.mint, "skip trade (sig decode)"),
                }
            }
            if !rows.is_empty() {
                match TradeRepo::insert_batch(&self.pool, &rows).await {
                    Ok(n) => tracing::debug!(rows = n, "flushed trades"),
                    Err(e) => {
                        warn!(?e, count = rows.len(), "trade bulk insert failed — retrying per-row");
                        for row in &rows {
                            if let Err(e) =
                                TradeRepo::insert_batch(&self.pool, std::slice::from_ref(row)).await
                            {
                                warn!(?e, "per-row trade insert failed — dropping row for this flush");
                            }
                        }
                    }
                }
            }
        }

        // Raw txs.
        if !raws.is_empty() {
            match RawTxRepo::insert_batch(&self.pool, &raws).await {
                Ok(n) => tracing::debug!(rows = n, "flushed raw_txs"),
                Err(e) => {
                    warn!(?e, count = raws.len(), "raw_tx bulk insert failed — retrying per-row");
                    for row in &raws {
                        if let Err(e) =
                            RawTxRepo::insert_batch(&self.pool, std::slice::from_ref(row)).await
                        {
                            warn!(?e, "per-row raw_tx insert failed — dropping row for this flush");
                        }
                    }
                }
            }
        }

        self.heartbeat.stamp();
    }

    /// Intern a wallet address → `wallet_dict` id, memoized. On a cache hit no DB
    /// round-trip occurs.
    async fn wallet_ref(&mut self, address: &str) -> anyhow::Result<i32> {
        if let Some(id) = self.wallet_cache.get(address) {
            return Ok(*id);
        }
        let id = WalletDictRepo::intern(&self.pool, address).await?;
        self.wallet_cache.insert(address.to_string(), id);
        Ok(id)
    }
}
