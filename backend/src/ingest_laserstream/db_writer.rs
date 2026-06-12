use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::future::join_all;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{
    models::{token::Token, trade::Trade, transaction::RawTransaction},
    state::{token_metrics::compute_is_rugged, trade_signals::TradeSignals},
    storage::repositories::{
        token_info_repo::TokenInfoRepo, token_repo::TokenRepo, trade_repo::TradeRepo,
        transaction_repo::TransactionRepo, wallet_repo::WalletRepo,
    },
};

const BATCH_MAX: usize = 64;
const FLUSH_INTERVAL_MS: u64 = 25;

/// Async persistence queue — never blocks the ingest hot path.
#[derive(Debug)]
pub enum DbWriteOp {
    Raw(Arc<RawTransaction>),
    Token(Token),
    Wallet(String),
    Trade(Trade),
    Metrics(TokenMetricsWrite),
    Migration { mint: String },
}

#[derive(Debug, Clone)]
pub struct TokenMetricsWrite {
    pub mint: String,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<chrono::DateTime<Utc>>,
    pub age_seconds: Option<i64>,
    pub volume: f64,
    pub market_cap: Option<f64>,
    pub trade_count: i64,
    pub last_trade_at: Option<chrono::DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_migrated: bool,
    pub creator_wallet: String,
    pub recompute_rugged: bool,
}

pub struct DbWriter {
    pool: PgPool,
    /// Wakes live buy/sell confirm loops the instant a trade they're waiting on is
    /// persisted (see [`TradeSignals`]). Signalled only after the row is committed,
    /// so a woken waiter that reads the DB is guaranteed to see it.
    trade_signals: Arc<TradeSignals>,
}

impl DbWriter {
    pub fn new(pool: PgPool, trade_signals: Arc<TradeSignals>) -> Self {
        Self {
            pool,
            trade_signals,
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
        let token_repo = TokenRepo::new(self.pool.clone());
        let trade_repo = TradeRepo::new(self.pool.clone());
        let wallet_repo = WalletRepo::new(self.pool.clone());
        let tx_repo = TransactionRepo::new(self.pool.clone());
        let info_repo = TokenInfoRepo::new(self.pool.clone());

        // Partition the batch by type, collapsing redundant writes so a hot
        // token/wallet doesn't turn one flush into dozens of round-trips:
        //  • trades     → keep the last op per (tx_signature, leg_index)
        //  • metrics    → keep the last op per mint (each is a full snapshot;
        //                 the upsert is absolute, so the latest supersedes)
        //  • wallets    → unique addresses (touch only stamps `now`)
        //  • migrations → unique mints (idempotent OR-true)
        // Keeping the *last* op is exactly equivalent to applying them in order.
        let mut tokens: Vec<Token> = Vec::new();
        let mut raws: Vec<Arc<RawTransaction>> = Vec::new();
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
        // Low volume, so kept per-row: one malformed token can't drop the rest.
        for token in &tokens {
            if let Err(e) = token_repo.insert(token).await {
                warn!("DbWriter: token {}: {e}", token.mint_address);
            }
        }

        // Raw transactions — one multi-row insert, falling back to per-row on
        // error so a single bad row doesn't drop the whole batch.
        if let Err(e) = tx_repo.insert_many(&raws, "grpc").await {
            warn!("DbWriter: raw bulk insert failed ({e}); retrying per-row");
            for tx in &raws {
                if let Err(e) = tx_repo.insert(tx, "grpc").await {
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

        // Metrics — the (expensive) rugged recompute now runs once per unique
        // mint instead of once per trade. Trades above are already persisted, so
        // the rugged reads see this batch's trades. Each mint's recompute+upsert
        // is independent, so run them concurrently (bounded by the PgPool's
        // connection cap) rather than serially across the batch's unique mints.
        let metric_writes = metrics.values().map(|m| {
            let trade_repo = &trade_repo;
            let info_repo = &info_repo;
            async move {
                let is_rugged = if m.recompute_rugged {
                    compute_is_rugged(trade_repo, &m.mint, &m.creator_wallet, m.last_trade_at).await
                } else {
                    false
                };
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
                        is_rugged,
                        m.is_migrated,
                    )
                    .await
                {
                    warn!("DbWriter: metrics {}: {e}", m.mint);
                }
            }
        });
        join_all(metric_writes).await;

        // Migrations — idempotent, low volume.
        for mint in &migrations {
            if let Err(e) = info_repo.update_migration_status(mint, true).await {
                warn!("DbWriter: migration {mint}: {e}");
            }
        }
    }
}
