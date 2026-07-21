//! Batched DB persistence for the ingest pipeline.
//!
//! Receives [`DbWriteOp`] items from the consumer, accumulates them into batches,
//! and flushes on a timer or when the batch hits capacity. A separate task from the
//! hot-path consumer — the consumer never blocks on DB I/O.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};

use trading_core::{
    models::{raw_tx::RawTx, token::Token, trade::Trade},
    state::token_metrics::TokenMetricsWrite,
    state::trade_signals::TradeSignals,
    storage::repositories::{
        raw_tx_repo::RawTxRepo, token_info_repo::TokenInfoRepo, token_repo::TokenRepo,
        trade_repo::TradeRepo, wallet_repo::WalletRepo,
    },
};

use super::watchdog::DbHeartbeat;

const BATCH_MAX: usize = 1000;
const FLUSH_INTERVAL_MS: u64 = 150;

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

/// Pre-built raw-tx record (from [`ingest_laserstream::event::RawTx`]). Carries
/// the verbatim protobuf wire bytes — the consumer does no decode/re-encode.
#[derive(Debug)]
pub struct RawBlobJob {
    pub signature: Vec<u8>,
    pub slot: u64,
    pub tx_index: u32,
    pub block_time: DateTime<Utc>,
    pub payload: Vec<u8>,
}

// ── DbWriter ──────────────────────────────────────────────────────────────────

pub struct DbWriter {
    token_repo: TokenRepo,
    trade_repo: TradeRepo,
    wallet_repo: WalletRepo,
    raw_tx_repo: RawTxRepo,
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
            raw_tx_repo: RawTxRepo::new(pool.clone()),
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

        // Raw transactions — payload already encoded by the consumer. source=0 (live).
        if !raws.is_empty() {
            let raw_txs: Vec<RawTx> = raws
                .into_iter()
                .map(|job| RawTx {
                    tx_signature: job.signature,
                    slot: job.slot as i64,
                    block_time: job.block_time,
                    tx_index: job.tx_index as i32,
                    payload: job.payload,
                    source: 0,
                })
                .collect();
            if let Err(e) = self.raw_tx_repo.insert_many(&raw_txs).await {
                warn!("DbWriter: raw bulk insert failed ({e}); retrying per-row");
                for tx in &raw_txs {
                    if let Err(e) = self.raw_tx_repo.insert(tx).await {
                        error!("DbWriter: raw tx slot={} idx={}: {e}", tx.slot, tx.tx_index);
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

        // Trades — notify only rows that actually persisted. `TradeSignals`
        // promises a queryable committed row; waking waiters after a failed
        // insert makes sell-confirm poll missing data.
        let trades: Vec<Trade> = trades.into_values().collect();
        let persisted: Vec<&Trade> = match self.trade_repo.insert_many(&trades).await {
            Ok(()) => trades.iter().collect(),
            Err(e) => {
                warn!("DbWriter: trade bulk insert failed ({e}); retrying per-row");
                let mut ok = Vec::with_capacity(trades.len());
                for t in &trades {
                    match self.trade_repo.insert(t).await {
                        Ok(()) => ok.push(t),
                        Err(e) => {
                            warn!("DbWriter: trade {}#{}: {e}", t.tx_signature, t.leg_index)
                        }
                    }
                }
                ok
            }
        };

        for t in persisted {
            self.trade_signals.notify(&t.wallet_address, &t.mint_address);
        }

        // Metrics — one batched multi-row upsert instead of a per-mint fan-out. The
        // old `buffer_unordered` path held several pool connections at once and issued
        // one round-trip per distinct mint; under a saturated pool that was the ingest
        // write most likely to time out `acquire()`. Fall back to per-row on failure so
        // one bad row can't drop the whole batch (mirrors tokens/raws/trades above).
        if !metrics.is_empty() {
            let rows: Vec<TokenMetricsWrite> = metrics.into_values().collect();
            if let Err(e) = self.info_repo.upsert_metrics_many(&rows).await {
                warn!("DbWriter: metrics bulk upsert failed ({e}); retrying per-row");
                for m in &rows {
                    if let Err(e) = self
                        .info_repo
                        .upsert_metrics(
                            &m.mint,
                            m.ath_price,
                            m.ath_timestamp,
                            m.age_seconds,
                            m.volume_sol,
                            m.market_cap,
                            m.trade_count,
                            m.last_trade_at,
                            m.current_price,
                            m.is_dead,
                            m.is_migrated,
                            m.lifetime_secs,
                            m.first_slot_buy_sol,
                            m.first_slot_sell_sol,
                        )
                        .await
                    {
                        warn!("DbWriter: metrics {}: {e}", m.mint);
                    }
                }
            }
        }

        // Migrations.
        for mint in &migrations {
            if let Err(e) = self.info_repo.update_migration_status(mint, true).await {
                warn!("DbWriter: migration {mint}: {e}");
            }
        }

        self.heartbeat.stamp();
    }
}
