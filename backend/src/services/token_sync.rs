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
    config::constants::{PUMP_SWAP_PROGRAM_ID, WSOL_MINT},
    ingest::decoder::{DecodeOutput, HeliusDecoder},
    models::{
        events::InternalEvent,
        token::Token,
        trade::Trade,
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
    /// When true, only fetch transactions newer than the last saved trade.
    pub incremental: bool,
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

/// Derive the canonical PumpSwap pool address for a migrated pump.fun token.
///
/// Migrated coins use a fixed pool layout: the pool's creator is the pump
/// program PDA `["pool-authority", mint]`, and the pool itself is the PumpSwap
/// PDA `["pool", 0u16, pool_authority, base_mint, WSOL]` (canonical index 0,
/// WSOL quote mint).
pub fn derive_pump_swap_pool(mint: &str, pump_program_id: &str) -> anyhow::Result<String> {
    let mint_pk = Pubkey::from_str(mint).context("invalid mint pubkey")?;
    let pump = Pubkey::from_str(pump_program_id).context("invalid pump program id")?;
    let swap = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).context("invalid pump swap program id")?;
    let wsol = Pubkey::from_str(WSOL_MINT).context("invalid wsol mint")?;

    let (authority, _) =
        Pubkey::find_program_address(&[b"pool-authority", mint_pk.as_ref()], &pump);
    let index: u16 = 0;
    let (pool, _) = Pubkey::find_program_address(
        &[
            b"pool",
            &index.to_le_bytes(),
            authority.as_ref(),
            mint_pk.as_ref(),
            wsol.as_ref(),
        ],
        &swap,
    );
    Ok(pool.to_string())
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

    // For incremental syncs, stop paging once we reach the last saved
    // bonding-curve trade (AMM trades are tracked separately, below).
    let until_signature: Option<String> = if req.incremental {
        TradeRepo::new(ctx.db.clone())
            .latest_signature(&mint, "curve")
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?
    } else {
        None
    };

    let signatures = rpc
        .get_all_signatures(&bonding_curve, until_signature.as_deref(), |page, total| {
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
                                .touch_last_seen(&e.token.creator_wallet, now)
                                .await;
                        }
                        InternalEvent::TradeExecuted(e) if e.trade.mint_address == mint => {
                            if skip_trades {
                                continue;
                            }
                            let _ = trade_repo.insert(&e.trade).await;
                            let now = Utc::now();
                            let _ = wallet_repo
                                .touch_last_seen(&e.trade.wallet_address, now)
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

    let is_migrated = db_migrated || prior_migrated || migrate_slot.is_some();

    // ── Post-migration AMM (PumpSwap) trades ─────────────────────────────────
    // Bonding-curve signatures never include PumpSwap swaps (the curve account
    // isn't touched once trading moves to the pool), so fetch them separately.
    if req.include_post_migrate && is_migrated {
        sync_amm_trades(
            &rpc,
            &decoder,
            &ctx.pump_program_id,
            &mint,
            req.incremental,
            &trade_repo,
            &tx_repo,
            &wallet_repo,
            &progress_tx,
        )
        .await?;
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

    // Re-read after the AMM pass so metrics include any newly inserted swaps.
    let trades = trade_repo
        .find_by_mint_all(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let mut state = TokenState::new(token);
    state.trades = trades.clone();
    state.is_migrated = is_migrated;

    token_metrics::recompute_token_state(&mut state);

    let metrics = metrics_from_state(&mint, &state, true);
    let rugged = token_metrics::compute_is_rugged(&trade_repo, &metrics).await;
    write_metrics(&info_repo, &metrics, rugged).await?;

    ctx.token_cache.insert(mint.clone(), state.clone());

    Ok(SyncOutput { state, trades })
}

/// Fetch and persist post-migration PumpSwap swaps for `mint`.
///
/// Derives the canonical pool, pages its signatures (resuming from the last
/// saved AMM trade when `incremental`), decodes Buy/Sell events, and inserts
/// them as `venue = "amm"` trades. A missing pool (not yet created on-chain)
/// is treated as "nothing to do", not an error.
#[allow(clippy::too_many_arguments)]
async fn sync_amm_trades(
    rpc: &HeliusRpc,
    decoder: &HeliusDecoder,
    pump_program_id: &str,
    mint: &str,
    incremental: bool,
    trade_repo: &TradeRepo,
    tx_repo: &TransactionRepo,
    wallet_repo: &WalletRepo,
    progress_tx: &mpsc::Sender<String>,
) -> Result<(), SyncError> {
    let pool = derive_pump_swap_pool(mint, pump_program_id)
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let exists = rpc
        .account_exists(&pool)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    if !exists {
        return Ok(());
    }

    let until: Option<String> = if incremental {
        trade_repo
            .latest_signature(mint, "amm")
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?
    } else {
        None
    };

    send_progress(progress_tx, "fetching_signatures", 0, 0, "Fetching AMM pool signatures").await?;

    let signatures = rpc
        .get_all_signatures(&pool, until.as_deref(), |page, total| {
            let line = serde_json::to_string(&SyncProgressEvent {
                event_type: "progress",
                stage: "fetching_signatures".into(),
                current: page as u64,
                total: 0,
                message: format!("Fetched {total} AMM signatures (page {page})"),
            })
            .unwrap_or_default()
                + "\n";
            let _ = progress_tx.try_send(line);
        })
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let fetched = fetch_transactions(rpc, &signatures, progress_tx).await?;

    send_progress(
        progress_tx,
        "processing",
        0,
        fetched.len() as u64,
        "Decoding AMM trades",
    )
    .await?;

    for entry in &fetched {
        if let Some((raw_tx, trades)) = decoder.decode_pump_swap_result(&entry.result, mint, &pool) {
            let _ = tx_repo.insert(&raw_tx).await;
            let now = Utc::now();
            for trade in trades {
                let _ = wallet_repo.touch_last_seen(&trade.wallet_address, now).await;
                let _ = trade_repo.insert(&trade).await;
            }
        }
    }

    Ok(())
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

#[cfg(test)]
mod amm_verification {
    //! On-chain verification of PumpSwap pool derivation + event decoding.
    //!
    //! Ignored by default (needs network + a Helius key). Run with:
    //!   HELIUS_RPC_URL="<url>" cargo test -p backend amm_pool_derivation -- --ignored --nocapture
    use super::*;
    use crate::config::constants::{PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID, WSOL_MINT};
    use crate::ingest::decoder::HeliusDecoder;
    use crate::services::helius_rpc::wrap_transaction_result;
    use serde_json::{json, Value};

    async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Value {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        client
            .post(url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["result"]
            .clone()
    }

    #[tokio::test]
    #[ignore = "requires HELIUS_RPC_URL and network access"]
    async fn amm_pool_derivation_and_event_decode() {
        let url = std::env::var("HELIUS_RPC_URL").expect("set HELIUS_RPC_URL");
        let client = reqwest::Client::new();
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());

        // 1. Grab recent PumpSwap program transactions (bounded).
        let sigs = rpc(
            &client,
            &url,
            "getSignaturesForAddress",
            json!([PUMP_SWAP_PROGRAM_ID, {"limit": 100, "commitment": "confirmed"}]),
        )
        .await;
        let sigs = sigs.as_array().expect("signatures array");

        let (mut swaps, mut canonical, mut non_canonical) = (0, 0, 0);
        let mut verified = 0;
        let mut mismatches: Vec<String> = Vec::new();

        for s in sigs {
            if !s["err"].is_null() {
                continue;
            }
            let sig = s["signature"].as_str().unwrap();
            let tx = rpc(
                &client,
                &url,
                "getTransaction",
                json!([sig, {"encoding":"jsonParsed","commitment":"confirmed","maxSupportedTransactionVersion":0}]),
            )
            .await;
            if tx.is_null() {
                continue;
            }

            let logs: Vec<&str> = tx["meta"]["logMessages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .collect();

            // Only consider real Buy/Sell swaps (skip liquidity ops, etc.).
            let Some(event_pool) = first_swap_event_pool(&logs) else {
                continue;
            };
            swaps += 1;

            // base mint = the non-WSOL SPL mint touched by the swap.
            let base_mint = tx["meta"]["postTokenBalances"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(tx["meta"]["preTokenBalances"].as_array().into_iter().flatten())
                .filter_map(|b| b["mint"].as_str())
                .find(|m| *m != WSOL_MINT)
                .map(|m| m.to_string());
            let Some(base_mint) = base_mint else { continue };

            // 2. Derive the pool purely from the mint and compare against the
            //    pool the on-chain event actually used.
            let derived = derive_pump_swap_pool(&base_mint, PUMP_FUN_PROGRAM_ID).unwrap();

            if derived != event_pool {
                // Non-canonical pool (user-created PumpSwap pool, non-zero
                // index, or non-pump token) — not something we sync.
                non_canonical += 1;
                if base_mint.ends_with("pump") {
                    // Heuristic: a pump.fun mint that DOESN'T match derivation
                    // would indicate a real bug — surface it.
                    mismatches.push(format!("{base_mint} event={event_pool} derived={derived}"));
                }
                continue;
            }
            canonical += 1;

            // 3. Production decode must succeed and produce sane values.
            let wrapped = wrap_transaction_result(sig, &tx);
            let (_raw, trades) = decoder
                .decode_pump_swap_result(&wrapped, &base_mint, &derived)
                .expect("canonical swap must decode");
            let t = &trades[0];
            assert_eq!(t.venue, "amm");
            assert!(t.sol_amount > 0.0 && t.token_amount > 0.0);
            if verified < 3 {
                println!(
                    "OK mint={base_mint}\n   derived_pool={derived}\n   {:?} sol={:.6} tokens_raw={} price={:.3e} wallet={}",
                    t.trade_type, t.sol_amount, t.token_amount, t.price_per_token, t.wallet_address
                );
            }
            verified += 1;
        }

        println!(
            "swaps_seen={swaps} canonical={canonical} non_canonical={non_canonical} verified={verified}"
        );
        assert!(
            mismatches.is_empty(),
            "pump.fun mints failed pool derivation (likely a bug):\n{}",
            mismatches.join("\n")
        );
        assert!(
            verified > 0,
            "no canonical pump swaps in sample — increase limit or target a known migrated mint"
        );
    }

    /// Parse the `pool` pubkey out of the first PumpSwap Buy/Sell event in the
    /// logs, independent of the production decoder, for cross-checking.
    /// Layout after the 8-byte discriminator: timestamp(i64) + 13×u64 = 112
    /// bytes, then pool(32).
    fn first_swap_event_pool(logs: &[&str]) -> Option<String> {
        for log in logs {
            let Some(encoded) = log.strip_prefix("Program data: ") else {
                continue;
            };
            let bytes = match base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                encoded,
            ) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if bytes.len() < 152 {
                continue;
            }
            let disc = &bytes[..8];
            if disc == crate::config::constants::PUMP_SWAP_BUY_EVENT_DISCRIMINATOR
                || disc == crate::config::constants::PUMP_SWAP_SELL_EVENT_DISCRIMINATOR
            {
                return Some(bs58::encode(&bytes[120..152]).into_string());
            }
        }
        None
    }
}
