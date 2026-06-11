use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{
    models::{token::Token, trade::Trade, transaction::RawTransaction},
    state::token_metrics::compute_is_rugged,
    storage::repositories::{
        token_info_repo::TokenInfoRepo, token_repo::TokenRepo, trade_repo::TradeRepo,
        transaction_grpc_repo::TransactionGrpcRepo, wallet_repo::WalletRepo,
    },
};

const BATCH_MAX: usize = 64;
const FLUSH_INTERVAL_MS: u64 = 25;

/// Async persistence queue — never blocks the ingest hot path.
#[derive(Debug)]
pub enum DbWriteOp {
    Raw(RawTransaction),
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
}

impl DbWriter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
        let ops: Vec<DbWriteOp> = batch.drain(..).collect();
        let token_repo = TokenRepo::new(self.pool.clone());
        let trade_repo = TradeRepo::new(self.pool.clone());
        let wallet_repo = WalletRepo::new(self.pool.clone());
        let tx_repo = TransactionGrpcRepo::new(self.pool.clone());
        let info_repo = TokenInfoRepo::new(self.pool.clone());

        for op in ops {
            match op {
                DbWriteOp::Raw(tx) => {
                    if let Err(e) = tx_repo.insert(&tx).await {
                        error!("DbWriter: raw tx {}: {e}", tx.signature);
                    }
                }
                DbWriteOp::Token(token) => {
                    if let Err(e) = token_repo.insert(&token).await {
                        warn!("DbWriter: token {}: {e}", token.mint_address);
                    }
                }
                DbWriteOp::Wallet(addr) => {
                    let now = Utc::now();
                    if let Err(e) = wallet_repo.touch_last_seen(&addr, now).await {
                        warn!("DbWriter: wallet touch {addr}: {e}");
                    }
                }
                DbWriteOp::Trade(trade) => {
                    if let Err(e) = trade_repo.insert(&trade).await {
                        warn!(
                            "DbWriter: trade {}#{}: {e}",
                            trade.tx_signature, trade.leg_index
                        );
                    }
                }
                DbWriteOp::Migration { mint } => {
                    if let Err(e) = info_repo.update_migration_status(&mint, true).await {
                        warn!("DbWriter: migration {mint}: {e}");
                    }
                }
                DbWriteOp::Metrics(m) => {
                    let is_rugged = if m.recompute_rugged {
                        compute_is_rugged(&trade_repo, &m.mint, &m.creator_wallet, m.last_trade_at)
                            .await
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
            }
        }
    }
}
