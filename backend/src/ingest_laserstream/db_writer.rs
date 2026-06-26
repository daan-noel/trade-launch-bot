use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};
use uuid::Uuid;

use super::adapter::build_raw_blob;
use super::proto::geyser::SubscribeUpdateTransaction;
use crate::{
    models::{token::Token, trade::Trade, transaction::RawTransaction},
    state::ingest_health::IngestHeartbeat,
    state::token_metrics::TokenMetricsWrite,
    state::trade_signals::TradeSignals,
    storage::repositories::{
        token_info_repo::TokenInfoRepo, token_repo::TokenRepo, trade_repo::TradeRepo,
        transaction_repo::TransactionRepo, wallet_repo::WalletRepo,
    },
};

// Larger batches + longer interval → fewer, bigger multi-row inserts → fewer
// fsyncs per second (the dominant cost on the 2vCPU/4GB box). With
// synchronous_commit=off the interval increase does not add durability risk.
const BATCH_MAX: usize = 1000;
const FLUSH_INTERVAL_MS: u64 = 150;
/// Max concurrent per-mint metric upsert tasks per flush. Bounded to match the
/// hot pool size (DB_MAX_CONNECTIONS=12): 6 metric upserts + trades/tokens/wallets
/// writes in the same flush keeps total concurrency well under the pool cap.
const METRIC_WRITE_CONCURRENCY: usize = 6;

/// Async persistence queue — never blocks the ingest hot path.
#[derive(Debug)]
pub enum DbWriteOp {
    Raw(RawBlobJob),
    Token(Token),
    Wallet(String),
    Trade(Trade),
    Metrics(TokenMetricsWrite),
    Migration { mint: String },
}

/// A persist-the-raw-blob job: the typed protobuf update plus the ingest-time
/// scalars. The heap-heavy Helius-shaped `Value` blob is synthesised from
/// `update` **here in the DbWriter** (off the ingest hot path) via
/// [`build_raw_blob`]; `received_at` is the ingest clock captured at decode, not
/// flush time, so the persisted row keeps the real receive timestamp.
#[derive(Debug)]
pub struct RawBlobJob {
    pub update: Arc<SubscribeUpdateTransaction>,
    pub signature: String,
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

pub struct DbWriter {
    /// Stateless repo handles built once at construction — each is just a cloned
    /// `PgPool` (an `Arc` bump). Built here rather than per-`flush` so the hot
    /// flush loop (up to ~40×/s) doesn't reconstruct all five every tick.
    token_repo: TokenRepo,
    trade_repo: TradeRepo,
    wallet_repo: WalletRepo,
    tx_repo: TransactionRepo,
    info_repo: TokenInfoRepo,
    /// Wakes live buy/sell confirm loops the instant a trade they're waiting on is
    /// persisted (see [`TradeSignals`]). Signalled only after the row is committed,
    /// so a woken waiter that reads the DB is guaranteed to see it.
    trade_signals: Arc<TradeSignals>,
    /// End-to-end ingest liveness clock read by the process watchdog. Stamped at
    /// the end of every [`flush`](Self::flush) — i.e. on each committed batch —
    /// because that is the one point where data actually lands in Postgres. A
    /// merely-slow writer keeps stamping (so the watchdog holds fire); a writer
    /// truly wedged on a hung DB `.await` never reaches the stamp, so the heartbeat
    /// ages and the watchdog restarts it. See [`crate::state::ingest_health`].
    heartbeat: IngestHeartbeat,
}

impl DbWriter {
    pub fn new(
        pool: PgPool,
        trade_signals: Arc<TradeSignals>,
        heartbeat: IngestHeartbeat,
    ) -> Self {
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
        // Repos are long-lived fields now — borrow them instead of rebuilding
        // five `PgPool` clones on every flush.
        let token_repo = &self.token_repo;
        let trade_repo = &self.trade_repo;
        let wallet_repo = &self.wallet_repo;
        let tx_repo = &self.tx_repo;
        let info_repo = &self.info_repo;

        // Partition the batch by type, collapsing redundant writes so a hot
        // token/wallet doesn't turn one flush into dozens of round-trips:
        //  • trades     → keep the last op per (tx_signature, leg_index)
        //  • metrics    → keep the last op per mint (each is a full snapshot;
        //                 the upsert is absolute, so the latest supersedes)
        //  • wallets    → unique addresses (touch only stamps `now`)
        //  • migrations → unique mints (idempotent OR-true)
        // Keeping the *last* op is exactly equivalent to applying them in order.
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

        // Tokens first — a same-batch create must land before its trades (FK).
        // Bulk insert (one round-trip); per-row fallback so one malformed token
        // can't drop the rest.
        if let Err(e) = token_repo.insert_many(&tokens).await {
            warn!("DbWriter: token bulk insert failed ({e}); retrying per-row");
            for token in &tokens {
                if let Err(e) = token_repo.insert(token).await {
                    warn!("DbWriter: token {}: {e}", token.mint_address);
                }
            }
        }

        // Raw transactions — synthesise the Helius-shaped blob from the protobuf
        // HERE (off the ingest hot path; the whole point of Tier B), then one
        // multi-row insert with a per-row fallback so one bad row can't drop the
        // batch. A job whose protobuf lacks the tx/message/meta we need yields no
        // blob and is skipped (it would have produced none on the hot path either).
        let raw_txs: Vec<Arc<RawTransaction>> = raws
            .iter()
            .filter_map(|job| {
                build_raw_blob(&job.update).map(|raw_data| {
                    Arc::new(RawTransaction {
                        id: Uuid::new_v4(),
                        signature: job.signature.clone(),
                        slot: job.slot,
                        block_time: job.block_time,
                        raw_data,
                        received_at: job.received_at,
                    })
                })
            })
            .collect();
        if let Err(e) = tx_repo.insert_many(&raw_txs, "live").await {
            warn!("DbWriter: raw bulk insert failed ({e}); retrying per-row");
            for tx in &raw_txs {
                if let Err(e) = tx_repo.insert(tx, "live").await {
                    error!("DbWriter: raw tx {}: {e}", tx.signature);
                }
            }
        }

        // Wallet last-seen — a single UPDATE over all unique addresses.
        if !wallets.is_empty() {
            let addrs: Vec<String> = wallets.into_iter().collect();
            if let Err(e) = wallet_repo.touch_last_seen_many(&addrs, Utc::now()).await {
                warn!("DbWriter: wallet touch ({} addrs): {e}", addrs.len());
            }
        }

        // Trades — one multi-row upsert (deduped above), per-row fallback.
        let trades: Vec<Trade> = trades.into_values().collect();
        if let Err(e) = trade_repo.insert_many(&trades).await {
            warn!("DbWriter: trade bulk insert failed ({e}); retrying per-row");
            for t in &trades {
                if let Err(e) = trade_repo.insert(t).await {
                    warn!("DbWriter: trade {}#{}: {e}", t.tx_signature, t.leg_index);
                }
            }
        }

        // Now the rows are queryable: wake any live confirm loop waiting on one of
        // these (wallet, mint) keys so it reads its fill immediately instead of
        // waiting out its next poll tick. A no-op for every key with no waiter,
        // which is virtually all of them.
        for t in &trades {
            self.trade_signals
                .notify(&t.wallet_address, &t.mint_address);
        }

        // Metrics — the dead-token verdict (`m.is_dead`) was computed in-memory at
        // metrics time (no DB scan), so the flush is now a plain upsert per unique
        // mint. Each upsert is independent, so run them concurrently — bounded by
        // `buffer_unordered` so a flush with hundreds of unique mints can't exhaust
        // the PgPool. `into_values` moves each `TokenMetricsWrite` straight into its
        // future — no per-metric clone — and collecting eagerly avoids the HRTB
        // inference error a lazy `.map()` fed into `buffer_unordered` trips.
        let metric_writes: Vec<_> = metrics.into_values().map(|m| {
            // Clone the long-lived repo (an `Arc<PgPool>` bump) into the owned future.
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

        // Migrations — idempotent, low volume.
        for mint in &migrations {
            if let Err(e) = info_repo.update_migration_status(mint, true).await {
                warn!("DbWriter: migration {mint}: {e}");
            }
        }

        // Real end-to-end progress: this batch's writes have all returned (a commit
        // round-trip completed, even if some per-row inserts logged errors — the
        // writer is alive, which is what the watchdog cares about). Stamp the
        // liveness heartbeat. `flush` is only called with a non-empty batch, so this
        // never fires on idle; a writer wedged on a hung DB `.await` never gets here.
        self.heartbeat.stamp();
    }
}
