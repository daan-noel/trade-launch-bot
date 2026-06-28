//! Batched DB persistence for the ingest pipeline.
//!
//! Receives [`DbWriteOp`] items from the consumer, accumulates them into batches,
//! and flushes on a timer or when the batch hits capacity. A separate task from the
//! hot-path consumer — the consumer never blocks on DB I/O.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};
use uuid::Uuid;

use trading_core::{
    models::{token::Token, trade::Trade},
    state::token_metrics::TokenMetricsWrite,
    state::trade_signals::TradeSignals,
    storage::repositories::{
        token_info_repo::TokenInfoRepo, token_repo::TokenRepo, trade_repo::TradeRepo,
        transaction_repo::TransactionRepo, wallet_repo::WalletRepo,
    },
};

use super::watchdog::DbHeartbeat;

const BATCH_MAX: usize = 1000;
const FLUSH_INTERVAL_MS: u64 = 150;
const METRIC_WRITE_CONCURRENCY: usize = 6;

// ── DbWriteOp ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum DbWriteOp {
    Raw(RawBlobJob),
    Token(Token),
    Wallet(String),
    Trade(Trade),
    Metrics(TokenMetricsWrite),
    Migration { mint: String },
}

/// Pre-built raw-tx blob (from [`ingest_laserstream::event::RawTx`]).
#[derive(Debug)]
pub struct RawBlobJob {
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub raw_data: Value,
}

// ── DbWriter ──────────────────────────────────────────────────────────────────

pub struct DbWriter {
    token_repo: TokenRepo,
    trade_repo: TradeRepo,
    wallet_repo: WalletRepo,
    tx_repo: TransactionRepo,
    info_repo: TokenInfoRepo,
    trade_signals: Arc<TradeSignals>,
    heartbeat: DbHeartbeat,
}

impl DbWriter {
    pub fn new(pool: PgPool, trade_signals: Arc<TradeSignals>, heartbeat: DbHeartbeat) -> Self {
        Self {
            token_repo: TokenRepo::new(pool.clone()),
            trade_repo: TradeRepo::new(pool.clone()),
            wallet_repo: WalletRepo::new(pool.clone()),
            tx_repo: TransactionRepo::new(pool.clone()),
            info_repo: TokenInfoRepo::new(pool),
            trade_signals,
            heartbeat,
        }
    }

    pub async fn run(self, mut rx: mpsc::Receiver<DbWriteOp>) {
        let mut batch = Vec::with_capacity(BATCH_MAX);
        let mut interval = tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));

        loop {
            tokio::select! {
                op = rx.recv() => {
                    match op {
                        Some(op) => {
                            batch.push(op);
                            if batch.len() >= BATCH_MAX {
                                self.flush(&mut batch).await;
                            }
                        }
                        None => {
                            if !batch.is_empty() {
                                self.flush(&mut batch).await;
                            }
                            return;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        self.flush(&mut batch).await;
                    }
                }
            }
        }
    }

    async fn flush(&self, batch: &mut Vec<DbWriteOp>) {
        use std::collections::{HashMap, HashSet};

        let ops: Vec<DbWriteOp> = batch.drain(..).collect();

        let mut tokens: Vec<Token> = Vec::new();
        let mut raws: Vec<RawBlobJob> = Vec::new();
        let mut wallets: HashSet<String> = HashSet::new();
        let mut trades: HashMap<(String, i32), Trade> = HashMap::new();
        let mut metrics: HashMap<String, TokenMetricsWrite> = HashMap::new();
        let mut migrations: HashSet<String> = HashSet::new();

        for op in ops {
            match op {
                DbWriteOp::Token(t) => tokens.push(t),
                DbWriteOp::Raw(tx) => raws.push(tx),
                DbWriteOp::Wallet(addr) => {
                    wallets.insert(addr);
                }
                DbWriteOp::Trade(t) => {
                    trades.insert((t.tx_signature.clone(), t.leg_index as i32), t);
                }
                DbWriteOp::Metrics(m) => {
                    metrics.insert(m.mint.clone(), m);
                }
                DbWriteOp::Migration { mint } => {
                    migrations.insert(mint);
                }
            }
        }

        // Tokens first (FK constraint: trades ref tokens).
        if let Err(e) = self.token_repo.insert_many(&tokens).await {
            warn!("DbWriter: token bulk insert failed ({e}); retrying per-row");
            for token in &tokens {
                if let Err(e) = self.token_repo.insert(token).await {
                    warn!("DbWriter: token {}: {e}", token.mint_address);
                }
            }
        }

        // Raw transactions — blob already built by the consumer.
        if !raws.is_empty() {
            use trading_core::models::transaction::RawTransaction;
            let raw_txs: Vec<_> = raws
                .into_iter()
                .map(|job| Arc::new(RawTransaction {
                    id: Uuid::new_v4(),
                    signature: job.signature,
                    slot: job.slot,
                    block_time: job.block_time,
                    raw_data: job.raw_data,
                    received_at: job.received_at,
                }))
                .collect();
            if let Err(e) = self.tx_repo.insert_many(&raw_txs, "live").await {
                warn!("DbWriter: raw bulk insert failed ({e}); retrying per-row");
                for tx in &raw_txs {
                    if let Err(e) = self.tx_repo.insert(tx, "live").await {
                        error!("DbWriter: raw tx {}: {e}", tx.signature);
                    }
                }
            }
        }

        // Wallets.
        if !wallets.is_empty() {
            let addrs: Vec<String> = wallets.into_iter().collect();
            if let Err(e) = self.wallet_repo.touch_last_seen_many(&addrs, Utc::now()).await {
                warn!("DbWriter: wallet touch ({} addrs): {e}", addrs.len());
            }
        }

        // Trades.
        let trades: Vec<Trade> = trades.into_values().collect();
        if let Err(e) = self.trade_repo.insert_many(&trades).await {
            warn!("DbWriter: trade bulk insert failed ({e}); retrying per-row");
            for t in &trades {
                if let Err(e) = self.trade_repo.insert(t).await {
                    warn!("DbWriter: trade {}#{}: {e}", t.tx_signature, t.leg_index);
                }
            }
        }

        for t in &trades {
            self.trade_signals.notify(&t.wallet_address, &t.mint_address);
        }

        // Metrics.
        let metric_writes: Vec<_> = metrics.into_values().map(|m| {
            let info_repo = self.info_repo.clone();
            async move {
                if let Err(e) = info_repo
                    .upsert_metrics(
                        &m.mint,
                        m.ath_price,
                        m.ath_timestamp,
                        m.age_seconds,
                        m.volume,
                        m.market_cap,
                        m.trade_count,
                        m.last_trade_at,
                        m.current_price,
                        m.is_dead,
                        m.is_migrated,
                        m.lifetime_secs,
                    )
                    .await
                {
                    warn!("DbWriter: metrics {}: {e}", m.mint);
                }
            }
        }).collect();
        stream::iter(metric_writes)
            .buffer_unordered(METRIC_WRITE_CONCURRENCY)
            .collect::<Vec<()>>()
            .await;

        // Migrations.
        for mint in &migrations {
            if let Err(e) = self.info_repo.update_migration_status(mint, true).await {
                warn!("DbWriter: migration {mint}: {e}");
            }
        }

        self.heartbeat.stamp();
    }
}
