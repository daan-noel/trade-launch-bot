use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::Context;
use chrono::Utc;
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc;

use crate::{
    ingest::decoder::{DecodeOutput, HeliusDecoder},
    models::{
        events::InternalEvent,
        token::Token,
        trade::Trade,
        wallet::Wallet,
    },
    state::{
        token_cache::{TokenCache, TokenState},
        token_metrics::{self, metrics_from_state},
    },
    storage::repositories::{
        token_info_repo::TokenInfoRepo,
        token_repo::TokenRepo,
        trade_repo::TradeRepo,
        transaction_repo::TransactionRepo,
        wallet_repo::WalletRepo,
    },
};

use super::helius_rpc::{HeliusRpc, SignatureEntry, wrap_transaction_result};

const TX_FETCH_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub stage: String,
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Clone)]
pub struct SyncOutput {
    pub state: TokenState,
    pub trades: Vec<Trade>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncCompleteEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub token: serde_json::Value,
    pub trades: serde_json::Value,
}

#[derive(Debug)]
pub enum SyncError {
    InvalidMint(String),
    NotPumpToken(String),
    NoTransactions(String),
    Internal(String),
}

impl SyncError {
    pub fn message(&self) -> &str {
        match self {
            SyncError::InvalidMint(m)
            | SyncError::NotPumpToken(m)
            | SyncError::NoTransactions(m)
            | SyncError::Internal(m) => m,
        }
    }
}

pub struct TokenSyncRequest {
    pub mint_address: String,
    pub include_post_migrate: bool,
}

pub struct TokenSyncContext {
    pub db: sqlx::PgPool,
    pub token_cache: Arc<TokenCache>,
    pub helius_rpc_url: String,
    pub pump_program_id: String,
}

pub fn derive_bonding_curve(mint: &str, pump_program_id: &str) -> anyhow::Result<String> {
    let mint_pk = Pubkey::from_str(mint).context("invalid mint pubkey")?;
    let program = Pubkey::from_str(pump_program_id).context("invalid pump program id")?;
    let (bc, _) = Pubkey::find_program_address(&[b"bonding-curve", mint_pk.as_ref()], &program);
    Ok(bc.to_string())
}

pub fn validate_mint_address(mint: &str) -> Result<(), SyncError> {
    let mint = mint.trim();
    if mint.is_empty() {
        return Err(SyncError::InvalidMint(
            "Mint address is required.".into(),
        ));
    }
    Pubkey::from_str(mint).map_err(|_| {
        SyncError::InvalidMint(
            "Not a valid mint address. Enter a Solana base58 public key.".into(),
        )
    })?;
    Ok(())
}

pub async fn preflight(
    rpc: &HeliusRpc,
    mint: &str,
    pump_program_id: &str,
) -> Result<String, SyncError> {
    validate_mint_address(mint)?;
    let mint = mint.trim();
    let bonding_curve = derive_bonding_curve(mint, pump_program_id).map_err(|e| {
        SyncError::InvalidMint(format!("Not a valid mint address: {e}"))
    })?;

    let exists = rpc
        .account_exists(&bonding_curve)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    if !exists {
        return Err(SyncError::NotPumpToken(
            "Not a valid Pump.fun token mint address. No bonding curve account found on-chain."
                .into(),
        ));
    }

    let has_txs = rpc
        .has_any_signature(&bonding_curve)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    if !has_txs {
        return Err(SyncError::NoTransactions(
            "No transactions found for this mint address.".into(),
        ));
    }

    Ok(bonding_curve.to_string())
}

pub async fn run_token_sync(
    ctx: TokenSyncContext,
    req: TokenSyncRequest,
    progress_tx: mpsc::Sender<String>,
) -> Result<SyncOutput, SyncError> {
    let mint = req.mint_address.trim().to_string();
    validate_mint_address(&mint)?;

    let rpc = HeliusRpc::new(ctx.helius_rpc_url.clone());
    let bonding_curve = preflight(&rpc, &mint, &ctx.pump_program_id).await?;

    send_progress(
        &progress_tx,
        "validating",
        1,
        1,
        "Bonding curve verified",
    )
    .await?;

    let decoder = HeliusDecoder::new(ctx.pump_program_id.clone());

    let signatures = rpc
        .get_all_signatures(&bonding_curve, |page, total| {
            let line = serde_json::to_string(&SyncProgressEvent {
                event_type: "progress",
                stage: "fetching_signatures".into(),
                current: page as u64,
                total: 0,
                message: format!("Fetched {total} signatures (page {page})"),
            })
            .unwrap_or_default()
                + "\n";
            let _ = progress_tx.try_send(line);
        })
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let sig_total = signatures.len() as u64;
    send_progress(
        &progress_tx,
        "fetching_signatures",
        sig_total,
        sig_total,
        &format!("{sig_total} signatures to process"),
    )
    .await?;

    let fetched = fetch_transactions(&rpc, &signatures, &progress_tx).await?;

    send_progress(
        &progress_tx,
        "processing",
        0,
        fetched.len() as u64,
        "Decoding and saving transactions",
    )
    .await?;

    let token_repo = TokenRepo::new(ctx.db.clone());
    let trade_repo = TradeRepo::new(ctx.db.clone());
    let tx_repo = TransactionRepo::new(ctx.db.clone());
    let wallet_repo = WalletRepo::new(ctx.db.clone());
    let info_repo = TokenInfoRepo::new(ctx.db.clone());

    let mut migrate_slot: Option<u64> = None;
    let mut token_record: Option<Token> = token_repo
        .find_by_mint(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    for (idx, entry) in fetched.iter().enumerate() {
        if idx > 0 && idx % 25 == 0 {
            send_progress(
                &progress_tx,
                "processing",
                idx as u64,
                fetched.len() as u64,
                &format!("Processed {idx} / {}", fetched.len()),
            )
            .await?;
        }

        let slot = entry.slot;
        let skip_trades = !req.include_post_migrate
            && migrate_slot.is_some_and(|ms| slot > ms);

        match decoder.decode_result(&entry.result) {
            DecodeOutput::Transaction { raw_tx, mut events } => {
                sort_sync_events(&mut events);

                let _ = tx_repo.insert(&raw_tx).await;

                for event in events {
                    match event {
                        InternalEvent::TokenCreated(e) if e.token.mint_address == mint => {
                            token_repo
                                .upsert(&e.token)
                                .await
                                .map_err(|e| SyncError::Internal(e.to_string()))?;
                            token_record = Some(e.token.clone());
                            let now = Utc::now();
                            let _ = wallet_repo
                                .upsert(&Wallet::new(e.token.creator_wallet.clone(), now))
                                .await;
                        }
                        InternalEvent::TradeExecuted(e) if e.trade.mint_address == mint => {
                            if skip_trades {
                                continue;
                            }
                            let _ = trade_repo.insert(&e.trade).await;
                            let now = Utc::now();
                            let _ = wallet_repo
                                .upsert(&Wallet::new(e.trade.wallet_address.clone(), now))
                                .await;
                        }
                        InternalEvent::TokenMigrated(e) if e.mint_address == mint => {
                            migrate_slot = Some(e.slot);
                            let _ = info_repo.update_migration_status(&mint, true).await;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    send_progress(&progress_tx, "recomputing", 1, 1, "Rebuilding metrics").await?;

    token_record = match token_record {
        Some(t) => Some(t),
        None => token_repo
            .find_by_mint(&mint)
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    };

    let token = token_record.ok_or_else(|| {
        SyncError::NotPumpToken(
            "Not a valid Pump.fun token mint address. Could not parse token creation.".into(),
        )
    })?;

    let trades = trade_repo
        .find_by_mint_all(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let db_migrated = info_repo
        .find_by_mint(&mint)
        .await
        .ok()
        .flatten()
        .map(|i| i.is_migrated)
        .unwrap_or(false);

    let prior_migrated = ctx
        .token_cache
        .get(&mint)
        .map(|e| e.is_migrated)
        .unwrap_or(false);

    let mut state = TokenState::new(token);
    state.trades = trades.clone();
    state.is_migrated = db_migrated || prior_migrated || migrate_slot.is_some();

    token_metrics::recompute_token_state(&mut state);

    let metrics = metrics_from_state(&mint, &state, true);
    let rugged = token_metrics::compute_is_rugged(&trade_repo, &metrics).await;
    write_metrics(&info_repo, &metrics, rugged).await?;

    ctx.token_cache.insert(mint.clone(), state.clone());

    Ok(SyncOutput { state, trades })
}

struct FetchedTx {
    slot: u64,
    result: serde_json::Value,
}

async fn fetch_transactions(
    rpc: &HeliusRpc,
    signatures: &[SignatureEntry],
    progress_tx: &mpsc::Sender<String>,
) -> Result<Vec<FetchedTx>, SyncError> {
    use futures_util::stream::{self, StreamExt};

    let total = signatures.len();
    let done = Arc::new(AtomicUsize::new(0));

    let rpc = rpc.clone();
    let results: Vec<Option<FetchedTx>> = stream::iter(signatures.iter().cloned())
        .map(|entry| {
            let progress_tx = progress_tx.clone();
            let done = done.clone();
            let rpc = rpc.clone();
            async move {
                let tx = rpc.get_transaction(&entry.signature).await.ok().flatten()?;
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 10 == 0 || n == total {
                    let line = serde_json::to_string(&SyncProgressEvent {
                        event_type: "progress",
                        stage: "fetching_transactions".into(),
                        current: n as u64,
                        total: total as u64,
                        message: format!("Downloaded {n} / {total} transactions"),
                    })
                    .unwrap_or_default()
                        + "\n";
                    let _ = progress_tx.try_send(line);
                }
                let result = wrap_transaction_result(&entry.signature, &tx);
                Some(FetchedTx {
                    slot: entry.slot,
                    result,
                })
            }
        })
        .buffer_unordered(TX_FETCH_CONCURRENCY)
        .collect()
        .await;

    let mut out: Vec<FetchedTx> = results.into_iter().flatten().collect();
    out.sort_by_key(|t| t.slot);
    Ok(out)
}

async fn send_progress(
    tx: &mpsc::Sender<String>,
    stage: &str,
    current: u64,
    total: u64,
    message: &str,
) -> Result<(), SyncError> {
    let line = serde_json::to_string(&SyncProgressEvent {
        event_type: "progress",
        stage: stage.to_string(),
        current,
        total,
        message: message.to_string(),
    })
    .map_err(|e| SyncError::Internal(e.to_string()))?
        + "\n";
    tx.send(line)
        .await
        .map_err(|_| SyncError::Internal("progress channel closed".into()))
}

fn sort_sync_events(events: &mut [InternalEvent]) {
    events.sort_by_key(|e| match e {
        InternalEvent::TokenCreated(_) => 0,
        InternalEvent::TokenMigrated(_) => 1,
        InternalEvent::TradeExecuted(_) => 2,
        InternalEvent::CreatorActivityDetected(_) => 3,
        InternalEvent::LiquidityAdded(_) => 4,
        InternalEvent::LiquidityRemoved(_) => 5,
    });
}

async fn write_metrics(
    info_repo: &TokenInfoRepo,
    m: &crate::ingest::db_writer::TokenMetricsWrite,
    is_rugged: bool,
) -> Result<(), SyncError> {
    info_repo
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
        .map_err(|e| SyncError::Internal(e.to_string()))
}
