use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{mpsc, Notify};

use crate::{
    config::constants::{PUMP_SWAP_PROGRAM_ID, WSOL_MINT},
    ingest_laserstream::{
        adapter::build_raw_blob,
        adapter_rpc::rpc_to_protobuf,
        decoder::{DecodeOutput, HeliusDecoder},
        proto::geyser::SubscribeUpdateTransaction,
    },
    models::{
        events::InternalEvent,
        token::Token,
        trade::Trade,
        transaction::RawTransaction,
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

/// Signatures per JSON-RPC batch request — one HTTP round-trip fetches this many
/// transactions instead of one request each. Cuts latency, not Helius credits
/// (billed per `getTransaction`). 100 is the common provider batch cap.
const TX_BATCH_SIZE: usize = 100;
/// Concurrent in-flight batches.
const TX_BATCH_CONCURRENCY: usize = 5;

/// Page size for `getTransactionsForAddress` (gTFA) full-mode backfill. 1000 is
/// the max and the cheapest per-tx point (10 credits per 100 returned txs, so a
/// full 1000-tx page is 0.1 credit/tx).
const GTFA_PAGE_LIMIT: usize = 1000;

/// Max watermark age for which a Fetch-New sync trusts the LaserStream replay
/// window over the RPC path. The server's replay window is ~24h; we stay
/// conservatively inside it so we never replay a slot the server has aged out
/// (which could otherwise skip the gap between the watermark and the earliest
/// still-replayable slot). Older watermarks fall back to the RPC path.
const REPLAY_WINDOW_SECS: i64 = 20 * 3600;

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
    /// Full synced history, separate from `state.trades`: the latter is rebuilt
    /// through the 50K-capped retention ring by `recompute_token_state`, so for
    /// high-volume mints it is a truncated view. Kept distinct so the sync-complete
    /// response carries the complete list (L11: the second copy is *not* redundant).
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

/// Page cap for the sync preview. Bounds RPC cost when enumerating full history
/// for the "total" count — beyond this the preview reports `*_capped = true` and
/// the UI shows e.g. "10000+". 10 pages × 1000 sigs = 10k transactions.
const PREVIEW_MAX_PAGES: usize = 10;

/// Lightweight estimate of how many transactions a sync would download for a
/// mint, computed from signatures only (no `getTransaction`). `new_*` counts
/// what "Fetch New" would pull (newer than the watermark); `total_*` counts the
/// full history "Fetch All" would pull, capped at [`PREVIEW_MAX_PAGES`].
#[derive(Debug, Clone, Serialize)]
pub struct SyncPreview {
    pub new_count: u64,
    pub new_capped: bool,
    pub total_count: u64,
    pub total_capped: bool,
    pub is_migrated: bool,
}

pub struct TokenSyncContext {
    pub db: sqlx::PgPool,
    pub token_cache: Arc<TokenCache>,
    pub helius_rpc_url: String,
    /// LaserStream gRPC endpoint + API key for the Fetch-New replay fast path.
    /// Empty URL ⇒ replay disabled; incremental syncs use the RPC path only.
    pub helius_laserstream_url: String,
    pub helius_api_key: String,
    pub pump_program_id: String,
    /// Shared pool → mint index. A sync that confirms a token is migrated
    /// registers its PumpSwap pool here so the live WS subscribes to it.
    pub pool_index: Arc<DashMap<String, String>>,
    /// Pinged after registering a new pool, waking the WS task to subscribe.
    pub pools_changed: Arc<Notify>,
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

    // Seed a {pool → mint} index so post-migration PumpSwap (AMM) swaps — which
    // carry the pool, not the base mint — resolve back to this mint, letting both
    // the curve and AMM loops share the single `decode_protobuf` path (AMM frames
    // route to `decode_amm_live_pb` via this index). Harmless for curve txs.
    let pool_index = Arc::new(DashMap::new());
    if let Ok(pool) = derive_pump_swap_pool(&mint, &ctx.pump_program_id) {
        pool_index.insert(pool, mint.clone());
    }
    let decoder = HeliusDecoder::new(ctx.pump_program_id.clone()).with_pool_index(pool_index);

    let info_repo = TokenInfoRepo::new(ctx.db.clone());

    // Per-venue signature watermarks from the last successful sync. Preferred
    // over the latest saved trade as the incremental `until` boundary so a token
    // that synced with zero trades still resumes instead of re-fetching all txs.
    // The slots + `last_synced_at` gate the LaserStream replay fast path below.
    let (last_synced_at, prev_curve_sig, prev_amm_sig, prev_curve_slot, prev_amm_slot) = info_repo
        .get_sync_watermark(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    // For incremental syncs, stop paging once we reach the last bonding-curve
    // signature we already have (AMM trades are tracked separately, below).
    let until_signature: Option<String> = if req.incremental {
        match prev_curve_sig.clone() {
            Some(sig) => Some(sig),
            None => TradeRepo::new(ctx.db.clone())
                .latest_signature(&mint, "curve")
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?,
        }
    } else {
        None
    };

    // Full ("Fetch All") backfills use the archival getTransactionsForAddress
    // (one cursor-paginated call returning full txs, ~0.1 credit/tx). Incremental
    // ("Fetch New") uses the signatures + dedup + batched-getTransaction path: it
    // only downloads the few genuinely-new txs, cheaper than paying gTFA's per-tx
    // rate over a whole range that live ingest mostly already saved.
    let (fetched, newest_curve_sig, newest_curve_slot) = if !req.incremental {
        let (txs, newest) = acquire_full_via_gtfa(&rpc, &bonding_curve, &progress_tx).await?;
        let newest_slot = txs.last().map(|t| t.slot as i64).or(prev_curve_slot);
        (txs, newest.or_else(|| prev_curve_sig.clone()), newest_slot)
    } else if let Some((txs, sig, slot)) = try_replay(
        &ctx,
        &bonding_curve,
        prev_curve_slot,
        last_synced_at,
        "curve",
        &progress_tx,
    )
    .await
    {
        // LaserStream replay served the new txs for zero Helius credits.
        let newest_slot = slot.map(|s| s as i64).or(prev_curve_slot);
        (txs, sig.or_else(|| prev_curve_sig.clone()), newest_slot)
    } else {
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

        // Newest curve signature + slot seen this run (signatures are
        // slot-ascending, so the last entry is newest). Falls back to the prior
        // watermark when nothing new was fetched, so a no-op incremental sync
        // keeps the boundary. The slot is stamped too so a future Fetch New can
        // use the replay fast path.
        let newest = signatures
            .last()
            .map(|e| e.signature.clone())
            .or_else(|| prev_curve_sig.clone());
        let newest_slot = signatures.last().map(|e| e.slot as i64).or(prev_curve_slot);

        let sig_total = signatures.len() as u64;
        send_progress(
            &progress_tx,
            "fetching_signatures",
            sig_total,
            sig_total,
            &format!("{sig_total} signatures to process"),
        )
        .await?;

        // Incremental syncs skip transactions already saved (e.g. by live ingest)
        // so we don't re-spend getTransaction credits on them. "Fetch All" via the
        // rpc path re-fetches everything so decoder fixes propagate via the trades
        // ON CONFLICT DO UPDATE — dedup is intentionally incremental-only.
        let to_fetch = if req.incremental {
            let candidates: Vec<String> =
                signatures.iter().map(|e| e.signature.clone()).collect();
            let saved = TradeRepo::new(ctx.db.clone())
                .saved_signatures(&mint, "curve", &candidates)
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?;
            let kept: Vec<SignatureEntry> = signatures
                .into_iter()
                .filter(|e| !saved.contains(&e.signature))
                .collect();
            let skipped = sig_total as usize - kept.len();
            if skipped > 0 {
                send_progress(
                    &progress_tx,
                    "fetching_transactions",
                    0,
                    kept.len() as u64,
                    &format!("Skipping {skipped} already-saved tx; downloading {}", kept.len()),
                )
                .await?;
            }
            kept
        } else {
            signatures
        };

        let fetched = fetch_transactions(&rpc, &to_fetch, &progress_tx).await?;
        (fetched, newest, newest_slot)
    };

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

    let mut migrate_slot: Option<u64> = None;
    let mut token_record: Option<Token> = token_repo
        .find_by_mint(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    // Decode the batch into accumulators, then bulk-persist once via
    // `persist_backfill`. Replaces the old per-row `let _ = insert(...)` loop that
    // swallowed every write error while the watermark (stamped below) advanced
    // regardless — silently and permanently skipping any row that failed to save.
    // Token-creation upsert and the migrate-slot discovery stay inline because the
    // loop's `skip_trades` gate depends on a migration seen earlier in slot order.
    let mut curve_txs: Vec<Arc<RawTransaction>> = Vec::new();
    let mut curve_trades: Vec<Trade> = Vec::new();
    let mut curve_wallets: Vec<String> = Vec::new();

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

        if let DecodeOutput::Transaction { raw_tx, mut events } =
            decoder.decode_protobuf(&entry.update, entry.block_time)
        {
            sort_sync_events(&mut events);

            curve_txs.push(persist_tx(&entry.update, &raw_tx));

            for event in events {
                match event {
                    InternalEvent::TokenCreated(e) if e.token.mint_address == mint => {
                        token_repo
                            .upsert(&e.token)
                            .await
                            .map_err(|e| SyncError::Internal(e.to_string()))?;
                        curve_wallets.push(e.token.creator_wallet.clone());
                        token_record = Some(e.token);
                    }
                    InternalEvent::TradeExecuted(mut e) if e.trade.mint_address == mint => {
                        if skip_trades {
                            continue;
                        }
                        e.trade.received_at = Utc::now();
                        curve_wallets.push(e.trade.wallet_address.clone());
                        curve_trades.push(e.trade);
                    }
                    InternalEvent::TokenMigrated(e) if e.mint_address == mint => {
                        migrate_slot = Some(e.slot);
                        if let Err(err) = info_repo.update_migration_status(&mint, true).await {
                            tracing::warn!(
                                "token_sync: update_migration_status failed for {mint}: {err}"
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    persist_backfill(
        &trade_repo,
        &tx_repo,
        &wallet_repo,
        "curve",
        &curve_txs,
        &curve_trades,
        &mut curve_wallets,
    )
    .await?;

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
    let (new_amm_sig, new_amm_slot) = if req.include_post_migrate && is_migrated {
        let (sig, slot) = sync_amm_trades(
            &ctx,
            &rpc,
            &decoder,
            &mint,
            req.incremental,
            prev_amm_sig.as_deref(),
            prev_amm_slot,
            last_synced_at,
            &trade_repo,
            &tx_repo,
            &wallet_repo,
            &progress_tx,
        )
        .await?;
        (sig.or_else(|| prev_amm_sig.clone()), slot.or(prev_amm_slot))
    } else {
        // AMM not synced this run — keep any existing AMM watermark.
        (prev_amm_sig.clone(), prev_amm_slot)
    };

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
    // Full history is required here: this rebuilds the token's aggregate metrics
    // from scratch, so it must NOT be capped (a bounded read would understate
    // volume/ATH). Cold manual-sync path — not the ingest hot path.
    let trades = trade_repo
        .find_by_mint_all(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let mut state = TokenState::new(token);
    // `trades` is also returned (uncapped) in `SyncOutput` below, while
    // `recompute_token_state` rebuilds `state.trades` through the 50K retention
    // ring — so the two are not interchangeable for high-volume mints and this
    // copy is genuinely required (L11: not a redundant clone). Cold sync path.
    state.trades = std::sync::Arc::new(trades.clone());
    state.is_migrated = is_migrated;

    token_metrics::recompute_token_state(&mut state);

    let metrics = metrics_from_state(&mint, &state);
    let rugged = token_metrics::compute_is_rugged(
        &trade_repo,
        &metrics.mint,
        &metrics.creator_wallet,
        metrics.last_trade_at,
    )
    .await;
    write_metrics(&info_repo, &metrics, rugged).await?;

    // Stamp the sync watermark so the next "Fetch new" resumes from here, and
    // mirror it onto the cached state for immediate display.
    let synced_at = Utc::now();
    info_repo
        .update_sync_watermark(
            &mint,
            synced_at,
            newest_curve_sig.as_deref(),
            new_amm_sig.as_deref(),
            newest_curve_slot,
            new_amm_slot,
        )
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    state.last_synced_at = Some(synced_at);

    ctx.token_cache.insert(mint.clone(), state.clone());

    // A synced migrated token is one the user wants to watch — register its pool
    // so the live WS subscribes immediately (rather than waiting for the periodic
    // revival sweep). The next reconnect re-prunes it if it's since gone quiet.
    if is_migrated {
        if let Ok(pool) = derive_pump_swap_pool(&mint, &ctx.pump_program_id) {
            if ctx.pool_index.insert(pool, mint.clone()).is_none() {
                ctx.pools_changed.notify_one();
            }
        }
    }

    Ok(SyncOutput { state, trades })
}

/// Estimate, without downloading transactions, how many txs a sync would fetch.
///
/// Counts signatures only (the cheap `fetching_signatures` half of a sync):
/// `new_*` resolves the same incremental `until` boundary `run_token_sync` uses
/// (watermark, then latest saved trade) and counts what's newer; `total_*`
/// enumerates full history up to [`PREVIEW_MAX_PAGES`]. AMM pool signatures are
/// included only when the caller asks for post-migrate trades and the token has
/// migrated, mirroring the real sync.
pub async fn preview_sync(
    ctx: TokenSyncContext,
    req: TokenSyncRequest,
) -> Result<SyncPreview, SyncError> {
    let mint = req.mint_address.trim().to_string();
    validate_mint_address(&mint)?;

    let rpc = HeliusRpc::new(ctx.helius_rpc_url.clone());
    let bonding_curve = derive_bonding_curve(&mint, &ctx.pump_program_id)
        .map_err(|e| SyncError::InvalidMint(format!("Not a valid mint address: {e}")))?;

    // No bonding curve on-chain → not a pump token; nothing to estimate.
    let exists = rpc
        .account_exists(&bonding_curve)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    if !exists {
        return Err(SyncError::NotPumpToken(
            "No bonding curve account found on-chain.".into(),
        ));
    }

    let info_repo = TokenInfoRepo::new(ctx.db.clone());
    let trade_repo = TradeRepo::new(ctx.db.clone());

    let (_, prev_curve_sig, prev_amm_sig, _, _) = info_repo
        .get_sync_watermark(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    // Curve: count what "Fetch New" would pull (newer than the watermark) and
    // what "Fetch All" would pull (full history, capped).
    let curve_until: Option<String> = match prev_curve_sig {
        Some(sig) => Some(sig),
        None => trade_repo
            .latest_signature(&mint, "curve")
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    };
    let (mut new_count, mut new_capped) = rpc
        .count_signatures(&bonding_curve, curve_until.as_deref(), PREVIEW_MAX_PAGES)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    // With no watermark, "new" already enumerated full history, so "total" is
    // identical — reuse it instead of paging getSignaturesForAddress again.
    let (mut total_count, mut total_capped) = match curve_until.as_deref() {
        None => (new_count, new_capped),
        Some(_) => rpc
            .count_signatures(&bonding_curve, None, PREVIEW_MAX_PAGES)
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    };

    let is_migrated = info_repo
        .find_by_mint(&mint)
        .await
        .ok()
        .flatten()
        .map(|i| i.is_migrated)
        .unwrap_or(false)
        || ctx.token_cache.get(&mint).map(|e| e.is_migrated).unwrap_or(false);

    // Post-migration AMM pool — only counted when the sync would include it.
    if req.include_post_migrate && is_migrated {
        let pool = derive_pump_swap_pool(&mint, &ctx.pump_program_id)
            .map_err(|e| SyncError::Internal(e.to_string()))?;
        let pool_exists = rpc
            .account_exists(&pool)
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;
        if pool_exists {
            let amm_until: Option<String> = match prev_amm_sig {
                Some(sig) => Some(sig),
                None => trade_repo
                    .latest_signature(&mint, "amm")
                    .await
                    .map_err(|e| SyncError::Internal(e.to_string()))?,
            };
            let (amm_new, amm_new_capped) = rpc
                .count_signatures(&pool, amm_until.as_deref(), PREVIEW_MAX_PAGES)
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?;
            // Same reuse as the curve path: no watermark ⇒ total == new.
            let (amm_total, amm_total_capped) = match amm_until.as_deref() {
                None => (amm_new, amm_new_capped),
                Some(_) => rpc
                    .count_signatures(&pool, None, PREVIEW_MAX_PAGES)
                    .await
                    .map_err(|e| SyncError::Internal(e.to_string()))?,
            };
            new_count += amm_new;
            new_capped = new_capped || amm_new_capped;
            total_count += amm_total;
            total_capped = total_capped || amm_total_capped;
        }
    }

    Ok(SyncPreview {
        new_count: new_count as u64,
        new_capped,
        total_count: total_count as u64,
        total_capped,
        is_migrated,
    })
}

/// Try to fetch new transactions for `account` from the LaserStream replay
/// window instead of the RPC path (`getSignaturesForAddress` + `getTransaction`).
///
/// Returns `None` — so the caller falls back to the RPC path — when replay is
/// ineligible (no LaserStream URL, no slot watermark yet, or the watermark is
/// older than [`REPLAY_WINDOW_SECS`]) or on any replay error. Returns
/// `Some((txs, newest_sig, newest_slot))` on success, where the values may be
/// empty/`None` if nothing new was in the window.
///
/// Replay costs zero Helius credits. The caller stamps the watermark to the
/// returned `newest_slot` (the max slot actually decoded), never to chain tip,
/// so a partial drain can never skip data permanently — the next Fetch New just
/// replays again from the same slot.
async fn try_replay(
    ctx: &TokenSyncContext,
    account: &str,
    prev_slot: Option<i64>,
    last_synced_at: Option<chrono::DateTime<Utc>>,
    venue: &str,
    progress_tx: &mpsc::Sender<String>,
) -> Option<(Vec<FetchedTx>, Option<String>, Option<u64>)> {
    if ctx.helius_laserstream_url.trim().is_empty() {
        return None; // replay not configured
    }
    // Need a slot boundary to replay from and a fresh-enough watermark to trust
    // the replay window still covers it.
    let from_slot = prev_slot? as u64;
    let synced_at = last_synced_at?;
    if (Utc::now() - synced_at).num_seconds() > REPLAY_WINDOW_SECS {
        return None; // too old to replay reliably → RPC path
    }

    let replayed = match crate::services::laserstream_replay::replay_account_from_slot(
        &ctx.helius_laserstream_url,
        &ctx.helius_api_key,
        account,
        from_slot,
        &ctx.pump_program_id,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("token_sync: {venue} LaserStream replay failed ({e}); falling back to RPC");
            return None;
        }
    };

    // Newest signature + slot among the replayed txs (slot-ascending). Replay
    // frames carry no on-chain blockTime (exactly as the live path), so their
    // backfilled trades use `now()` as block_time — unchanged from the prior
    // `Value` replay path, which fell back to `now()` too.
    let now = Utc::now();
    let mut newest_sig: Option<String> = None;
    let mut newest_slot: Option<u64> = None;
    let mut txs: Vec<FetchedTx> = Vec::with_capacity(replayed.len());
    for r in replayed {
        if newest_slot.map_or(true, |s| r.slot >= s) {
            newest_slot = Some(r.slot);
            newest_sig = update_signature(&r.update);
        }
        txs.push(FetchedTx {
            slot: r.slot,
            block_time: now,
            update: r.update,
        });
    }

    let _ = send_progress(
        progress_tx,
        "fetching_transactions",
        txs.len() as u64,
        txs.len() as u64,
        &format!(
            "Replayed {} new {venue} tx via LaserStream (0 Helius credits)",
            txs.len()
        ),
    )
    .await;

    Some((txs, newest_sig, newest_slot))
}

/// Fetch and persist post-migration PumpSwap swaps for `mint`.
///
/// Derives the canonical pool, pages its signatures (resuming from the last
/// known AMM signature when `incremental`), decodes Buy/Sell events, and inserts
/// them as `venue = "amm"` trades. A missing pool (not yet created on-chain) is
/// treated as "nothing to do", not an error.
///
/// Returns the newest AMM `(signature, slot)` seen this run (slot-ascending
/// order, so the last entry), or `(None, None)` when nothing new was fetched —
/// the caller uses them to advance the stored AMM watermark.
#[allow(clippy::too_many_arguments)]
async fn sync_amm_trades(
    ctx: &TokenSyncContext,
    rpc: &HeliusRpc,
    decoder: &HeliusDecoder,
    mint: &str,
    incremental: bool,
    prev_amm_sig: Option<&str>,
    prev_amm_slot: Option<i64>,
    last_synced_at: Option<chrono::DateTime<Utc>>,
    trade_repo: &TradeRepo,
    tx_repo: &TransactionRepo,
    wallet_repo: &WalletRepo,
    progress_tx: &mpsc::Sender<String>,
) -> Result<(Option<String>, Option<i64>), SyncError> {
    let pool = derive_pump_swap_pool(mint, &ctx.pump_program_id)
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let exists = rpc
        .account_exists(&pool)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    if !exists {
        return Ok((None, None));
    }

    // Prefer the stored AMM watermark over the latest saved AMM trade, so a
    // migrated token with no decoded AMM trades yet still resumes correctly.
    let until: Option<String> = if incremental {
        match prev_amm_sig {
            Some(sig) => Some(sig.to_string()),
            None => trade_repo
                .latest_signature(mint, "amm")
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?,
        }
    } else {
        None
    };

    send_progress(progress_tx, "fetching_signatures", 0, 0, "Fetching AMM pool signatures").await?;

    // Full backfill via archival gTFA (cheaper + faster); incremental tries the
    // LaserStream replay window first, then falls back to the signatures + dedup
    // path. Mirrors the curve path in run_token_sync.
    let (fetched, newest_amm_sig, newest_amm_slot) = if !incremental {
        let (txs, newest) = acquire_full_via_gtfa(rpc, &pool, progress_tx).await?;
        let newest_slot = txs.last().map(|t| t.slot as i64);
        (txs, newest, newest_slot)
    } else if let Some((txs, sig, slot)) =
        try_replay(ctx, &pool, prev_amm_slot, last_synced_at, "amm", progress_tx).await
    {
        // LaserStream replay served the new AMM txs for zero Helius credits.
        (txs, sig, slot.map(|s| s as i64))
    } else {
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

        let newest = signatures.last().map(|e| e.signature.clone());
        let newest_slot = signatures.last().map(|e| e.slot as i64);

        // Incremental-only dedup: skip AMM swaps already saved (e.g. by live
        // ingest) so we don't re-fetch them from Helius.
        let to_fetch = if incremental {
            let candidates: Vec<String> =
                signatures.iter().map(|e| e.signature.clone()).collect();
            let saved = trade_repo
                .saved_signatures(mint, "amm", &candidates)
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?;
            signatures
                .into_iter()
                .filter(|e| !saved.contains(&e.signature))
                .collect::<Vec<SignatureEntry>>()
        } else {
            signatures
        };

        let fetched = fetch_transactions(rpc, &to_fetch, progress_tx).await?;
        (fetched, newest, newest_slot)
    };

    send_progress(
        progress_tx,
        "processing",
        0,
        fetched.len() as u64,
        "Decoding AMM trades",
    )
    .await?;

    // Decode the whole batch first, then persist in bulk. The old per-row
    // `let _ = insert(...)` swallowed every write error yet the caller still
    // advanced the AMM watermark — so a transient DB failure permanently lost
    // those swaps. Here a bulk-insert failure is propagated, and the caller only
    // stamps the AMM watermark on `Ok`, so the next incremental sync re-pulls
    // whatever didn't persist.
    // `decode_amm_protobuf` resolves each PumpSwap swap's pool to `mint` via the
    // decoder's seeded {pool → mint} index, dropping swaps for any other pool. It
    // has no curve-priority gate (unlike `decode_protobuf`), so an aggregator tx
    // that also touches the curve program still yields our pool's AMM trade.
    let mut amm_txs: Vec<Arc<RawTransaction>> = Vec::new();
    let mut amm_trades: Vec<Trade> = Vec::new();
    let mut amm_wallets: Vec<String> = Vec::new();
    for entry in &fetched {
        if let DecodeOutput::Transaction { raw_tx, events } =
            decoder.decode_amm_protobuf(&entry.update, entry.block_time)
        {
            amm_txs.push(persist_tx(&entry.update, &raw_tx));
            for event in events {
                if let InternalEvent::TradeExecuted(mut e) = event {
                    e.trade.received_at = Utc::now();
                    amm_wallets.push(e.trade.wallet_address.clone());
                    amm_trades.push(e.trade);
                }
            }
        }
    }

    persist_backfill(
        trade_repo,
        tx_repo,
        wallet_repo,
        "amm",
        &amm_txs,
        &amm_trades,
        &mut amm_wallets,
    )
    .await?;

    Ok((newest_amm_sig, newest_amm_slot))
}

/// Bulk-persist a decoded backfill batch (raw txs, trades, touched wallets) for
/// one venue and **propagate** any write failure to the caller. Centralizes the
/// "never silently drop, never advance the watermark past a failed write" policy
/// shared by the curve loop in `run_token_sync` and `sync_amm_trades`. `wallets`
/// is sorted/deduped in place before the single bulk touch. Raw txs are tagged
/// `source='sync'` (token_sync backfill — both "Fetch All" via gTFA and "Fetch
/// New" via LaserStream replay); the live ingest pipeline tags its own
/// `source='live'`.
async fn persist_backfill(
    trade_repo: &TradeRepo,
    tx_repo: &TransactionRepo,
    wallet_repo: &WalletRepo,
    venue: &str,
    txs: &[Arc<RawTransaction>],
    trades: &[Trade],
    wallets: &mut Vec<String>,
) -> Result<(), SyncError> {
    tx_repo
        .insert_many(txs, "sync")
        .await
        .map_err(|e| SyncError::Internal(format!("{venue} tx bulk insert failed: {e}")))?;
    trade_repo
        .insert_many(trades)
        .await
        .map_err(|e| SyncError::Internal(format!("{venue} trade bulk insert failed: {e}")))?;
    wallets.sort();
    wallets.dedup();
    wallet_repo
        .touch_last_seen_many(wallets, Utc::now())
        .await
        .map_err(|e| SyncError::Internal(format!("{venue} wallet touch failed: {e}")))?;
    Ok(())
}

/// A fetched transaction lowered to the protobuf frame both decode paths share.
/// Every source — gTFA, per-sig `getTransaction` (both `encoding="base64"`), and
/// LaserStream replay (native protobuf) — produces this, so the decode loops run
/// the single `decode_protobuf` path. `block_time` is the real on-chain time
/// (RPC `blockTime`, or `now()` for replay frames which carry none).
struct FetchedTx {
    slot: u64,
    block_time: DateTime<Utc>,
    update: SubscribeUpdateTransaction,
}

/// Lower one wrapped RPC result (`wrap_transaction_result` shape, `base64`
/// encoding) to a [`FetchedTx`]. Returns `None` if the base64/bincode decode fails
/// — treated like the decoder ignoring it.
fn fetched_from_rpc(slot: u64, wrapped: &Value) -> Option<FetchedTx> {
    let block_time = wrapped
        .get("blockTime")
        .and_then(Value::as_i64)
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
        .unwrap_or_else(Utc::now);
    Some(FetchedTx {
        slot,
        block_time,
        update: rpc_to_protobuf(wrapped)?,
    })
}

/// Base58 signature of a lowered frame (for the sync watermark).
fn update_signature(update: &SubscribeUpdateTransaction) -> Option<String> {
    update
        .transaction
        .as_ref()
        .map(|i| bs58::encode(&i.signature).into_string())
}

/// Build the persisted [`RawTransaction`] for a decoded backfill tx.
///
/// `decode_protobuf` returns a carrier with `raw_data: Null` (the live path
/// synthesises the blob off-thread in the DbWriter); token_sync has no DbWriter,
/// so synthesise it here via the shared [`build_raw_blob`] — the same
/// Helius-shaped blob the live gRPC path persists, keeping `raw_transactions`
/// byte-consistent across sources for later analysis. `RawTransaction::new` stamps
/// `received_at` to now, while `block_time` stays the real on-chain time from the
/// carrier.
fn persist_tx(update: &SubscribeUpdateTransaction, raw_tx: &RawTransaction) -> Arc<RawTransaction> {
    Arc::new(RawTransaction::new(
        raw_tx.signature.clone(),
        raw_tx.slot,
        raw_tx.block_time,
        build_raw_blob(update).unwrap_or(Value::Null),
    ))
}

/// Acquire an address's full transaction history via the archival
/// `getTransactionsForAddress` (gTFA), cursor-paginated. Each returned item is
/// already shaped like a `getTransaction` result, so we wrap it into the decoder
/// shape directly — no second round-trip per signature. Returns the txs
/// (slot-ascending) plus the newest signature seen (for the sync watermark).
///
/// Used for full ("Fetch All") backfills only: ~0.1 credit/tx at a 1000-tx page
/// vs 1 credit/tx for per-sig `getTransaction`. Like the rpc "Fetch All" path it
/// returns every tx, so decoder fixes still propagate via the trades upsert.
async fn acquire_full_via_gtfa(
    rpc: &HeliusRpc,
    address: &str,
    progress_tx: &mpsc::Sender<String>,
) -> Result<(Vec<FetchedTx>, Option<String>), SyncError> {
    let mut out: Vec<FetchedTx> = Vec::new();
    let mut newest_sig: Option<String> = None;
    let mut newest_slot: u64 = 0;
    let mut token: Option<String> = None;
    let mut page = 0usize;

    loop {
        page += 1;
        // Oldest-first so the accumulated order is roughly chronological (still
        // sorted by slot at the end as a guarantee). `base64` encoding so each
        // item lowers to protobuf via `rpc_to_protobuf` (see `fetched_from_rpc`).
        let (data, next) = rpc
            .get_transactions_for_address_full_page_enc(
                address,
                "asc",
                GTFA_PAGE_LIMIT,
                token.as_deref(),
                "base64",
            )
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        for item in &data {
            let slot = item.get("slot").and_then(|v| v.as_u64()).unwrap_or(0);
            // base64 items expose no `transaction.signatures`; pass "" and let the
            // adapter recover the signature from the bincode tx, then read it back
            // off the lowered frame for the watermark.
            let Some(ft) = fetched_from_rpc(slot, &wrap_transaction_result("", item)) else {
                continue;
            };
            if slot >= newest_slot {
                if let Some(sig) = update_signature(&ft.update) {
                    newest_slot = slot;
                    newest_sig = Some(sig);
                }
            }
            out.push(ft);
        }

        let line = serde_json::to_string(&SyncProgressEvent {
            event_type: "progress",
            stage: "fetching_transactions".into(),
            current: out.len() as u64,
            total: out.len() as u64,
            message: format!("Downloaded {} transactions (page {page})", out.len()),
        })
        .unwrap_or_default()
            + "\n";
        let _ = progress_tx.try_send(line);

        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }

    out.sort_by_key(|t| t.slot);
    Ok((out, newest_sig))
}

async fn fetch_transactions(
    rpc: &HeliusRpc,
    signatures: &[SignatureEntry],
    progress_tx: &mpsc::Sender<String>,
) -> Result<Vec<FetchedTx>, SyncError> {
    use futures_util::stream::{self, StreamExt};

    let total = signatures.len();
    let done = Arc::new(AtomicUsize::new(0));

    // Chunk into JSON-RPC batches; each batch is one HTTP request fetching up to
    // TX_BATCH_SIZE transactions, with a few batches in flight at once.
    let batches: Vec<Vec<SignatureEntry>> = signatures
        .chunks(TX_BATCH_SIZE)
        .map(|c| c.to_vec())
        .collect();

    let rpc = rpc.clone();
    let results: Vec<Vec<FetchedTx>> = stream::iter(batches)
        .map(|batch| {
            let progress_tx = progress_tx.clone();
            let done = done.clone();
            let rpc = rpc.clone();
            async move {
                let sigs: Vec<String> = batch.iter().map(|e| e.signature.clone()).collect();

                // On a whole-batch HTTP failure, fall back to individual fetches
                // so one transient error doesn't drop up to TX_BATCH_SIZE txs.
                let txs = match rpc.get_transactions_batch(&sigs).await {
                    Ok(txs) => txs,
                    Err(_) => {
                        let mut indiv = Vec::with_capacity(sigs.len());
                        for sig in &sigs {
                            indiv.push(rpc.get_transaction(sig).await.ok().flatten());
                        }
                        indiv
                    }
                };

                let mut out = Vec::with_capacity(batch.len());
                for (entry, tx) in batch.iter().zip(txs.into_iter()) {
                    if let Some(tx) = tx {
                        if let Some(ft) =
                            fetched_from_rpc(entry.slot, &wrap_transaction_result(&entry.signature, &tx))
                        {
                            out.push(ft);
                        }
                    }
                }

                // Progress counts signatures processed (attempted), so it reaches
                // `total` even when some txs were missing/failed.
                let n = (done.fetch_add(batch.len(), Ordering::Relaxed) + batch.len()).min(total);
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

                out
            }
        })
        .buffer_unordered(TX_BATCH_CONCURRENCY)
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
    m: &crate::ingest_laserstream::db_writer::TokenMetricsWrite,
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
    use crate::ingest_laserstream::decoder::HeliusDecoder;
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

            // 3. Production decode (the live protobuf path) must succeed and
            //    produce sane values: fetch base64, lower to protobuf, and decode
            //    with a {pool → mint} index so it routes through decode_amm_live_pb
            //    — exactly how token_sync's AMM loop decodes.
            let tx_b64 = rpc(
                &client,
                &url,
                "getTransaction",
                json!([sig, {"encoding":"base64","commitment":"confirmed","maxSupportedTransactionVersion":0}]),
            )
            .await;
            let idx = std::sync::Arc::new(dashmap::DashMap::new());
            idx.insert(derived.clone(), base_mint.clone());
            let amm_decoder =
                HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string()).with_pool_index(idx);
            let update = crate::ingest_laserstream::adapter_rpc::rpc_to_protobuf(
                &wrap_transaction_result(sig, &tx_b64),
            )
            .expect("canonical swap must lower to protobuf");
            let trades: Vec<_> = match amm_decoder.decode_amm_protobuf(&update, Utc::now()) {
                DecodeOutput::Transaction { events, .. } => events
                    .into_iter()
                    .filter_map(|e| match e {
                        InternalEvent::TradeExecuted(t) => Some(t.trade),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            assert!(!trades.is_empty(), "canonical swap must decode");
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

    /// Live smoke test of token_sync's production decode path: pull one gTFA page
    /// in `encoding="base64"` (confirming the archival endpoint honors base64 +
    /// returns `meta.loadedAddresses` for versioned txs), lower each item via
    /// `rpc_to_protobuf`, decode with `decode_protobuf`, and assert it yields sane
    /// trades over the page. Run with:
    ///   HELIUS_RPC_URL="<url>" cargo test -p backend gtfa_base64_decodes_via_protobuf -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "requires HELIUS_RPC_URL with the archival API + network"]
    async fn gtfa_base64_decodes_via_protobuf() {
        use crate::ingest_laserstream::adapter_rpc::rpc_to_protobuf;
        use base64::{engine::general_purpose::STANDARD, Engine};
        use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};

        let url = std::env::var("HELIUS_RPC_URL").expect("set HELIUS_RPC_URL");
        let helius = HeliusRpc::new(url);
        let decoder = HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string());

        let (data, _token) = helius
            .get_transactions_for_address_full_page_enc(PUMP_FUN_PROGRAM_ID, "desc", 25, None, "base64")
            .await
            .expect("gTFA base64 call failed — endpoint may not honor encoding=base64");
        assert!(!data.is_empty(), "gTFA base64 returned no transactions");

        let (mut decoded, mut versioned_with_lut, mut trade_legs) = (0usize, 0usize, 0usize);
        for item in &data {
            let Some(tx_b64) = item["transaction"][0].as_str() else {
                continue;
            };
            let vtx: VersionedTransaction = bincode::deserialize(
                &STANDARD.decode(tx_b64).expect("gTFA base64 tx not decodable"),
            )
            .expect("gTFA base64 tx not bincode VersionedTransaction");

            // Confirm versioned txs carry loadedAddresses (the field the protobuf
            // decoder needs for correct account attribution).
            if matches!(vtx.message, VersionedMessage::V0(_)) {
                let lut = |k: &str| item["meta"]["loadedAddresses"][k].as_array().is_some_and(|a| !a.is_empty());
                if lut("writable") || lut("readonly") {
                    versioned_with_lut += 1;
                }
            }

            let Some(update) = rpc_to_protobuf(&wrap_transaction_result("", item)) else {
                continue;
            };
            if let DecodeOutput::Transaction { events, .. } =
                decoder.decode_protobuf(&update, Utc::now())
            {
                decoded += 1;
                for e in &events {
                    if let InternalEvent::TradeExecuted(t) = e {
                        assert!(
                            t.trade.sol_amount > 0.0 && t.trade.token_amount > 0.0,
                            "decoded trade has non-positive amounts: {:?}",
                            t.trade
                        );
                        trade_legs += 1;
                    }
                }
            }
        }

        println!(
            "decoded {decoded}/{} gTFA base64 txs ({versioned_with_lut} versioned w/ loadedAddresses), \
             {trade_legs} trade legs via decode_protobuf ✓",
            data.len()
        );
        assert!(trade_legs > 0, "no trades decoded from the gTFA page");
    }
}

#[cfg(test)]
mod backfill_persistence {
    //! DB-backed tests for `persist_backfill` — the helper both the curve loop and
    //! `sync_amm_trades` route their decoded batch through. Validates the Phase-0
    //! fix that replaced per-row `let _ = insert(...)` (swallowed errors, watermark
    //! advanced anyway) with a bulk insert whose failure propagates *before* the
    //! caller stamps the sync watermark.
    //!
    //! `#[ignore]`d like the other DB tests; run against a local Postgres:
    //!   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/meme_bot"
    //!   cargo test -p backend backfill_persistence -- --ignored --nocapture
    use super::*;
    use crate::models::trade::TradeType;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;
    use uuid::Uuid;

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()
    }

    fn uniq(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    fn raw_tx(sig: &str, slot: u64) -> Arc<RawTransaction> {
        Arc::new(RawTransaction::new(
            sig.to_string(),
            slot,
            Utc::now(),
            serde_json::json!({"signature": sig}),
        ))
    }

    fn trade(mint: &str, wallet: &str, sig: &str, slot: u64) -> Trade {
        Trade::new(
            mint.to_string(),
            wallet.to_string(),
            TradeType::Buy,
            0.5,
            1_000.0,
            sig.to_string(),
            slot,
            Utc::now(),
        )
    }

    async fn cleanup(pool: &PgPool, mint: &str, sigs: &[String]) {
        let _ = sqlx::query("DELETE FROM trades WHERE mint_address = $1")
            .bind(mint)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM raw_transactions WHERE signature = ANY($1)")
            .bind(sigs)
            .execute(pool)
            .await;
    }

    /// Happy path: a decoded batch is fully persisted — every tx and trade lands —
    /// and the call returns `Ok`, which is what lets the caller stamp the
    /// watermark. (The wallet touch is exercised too; `touch_last_seen_many` on an
    /// unknown address is a no-op UPDATE, so it neither errors nor needs the
    /// `wallet_profiles` FK chain seeded here.)
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn persist_backfill_writes_whole_batch_and_returns_ok() {
        let Some(pool) = test_pool().await else { return };
        let trade_repo = TradeRepo::new(pool.clone());
        let tx_repo = TransactionRepo::new(pool.clone());
        let wallet_repo = WalletRepo::new(pool.clone());

        let mint = uniq("MINT-pb-");
        let wallet = uniq("W-pb-");
        let sig_a = uniq("sig-a-");
        let sig_b = uniq("sig-b-");

        let txs = vec![raw_tx(&sig_a, 10), raw_tx(&sig_b, 11)];
        let trades = vec![
            trade(&mint, &wallet, &sig_a, 10),
            trade(&mint, &wallet, &sig_b, 11),
        ];
        let mut wallets = vec![wallet.clone(), wallet.clone()]; // dup → deduped inside

        let res = persist_backfill(
            &trade_repo,
            &tx_repo,
            &wallet_repo,
            "curve",
            &txs,
            &trades,
            &mut wallets,
        )
        .await;
        assert!(res.is_ok(), "happy path must return Ok: {res:?}");
        assert_eq!(wallets.len(), 1, "wallet list deduped before the bulk touch");

        let saved = trade_repo
            .find_by_mint_all(&mint)
            .await
            .expect("find trades");
        assert_eq!(saved.len(), 2, "both trades persisted");

        for sig in [&sig_a, &sig_b] {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM raw_transactions WHERE signature=$1)")
                    .bind(sig)
                    .fetch_one(&pool)
                    .await
                    .expect("check raw tx");
            assert!(exists, "raw tx {sig} persisted");
        }

        cleanup(&pool, &mint, &[sig_a, sig_b]).await;
    }

    /// Failure path: a constraint-violating trade (illegal `venue`) makes the bulk
    /// insert error, and `persist_backfill` returns `Err`. The caller invokes it as
    /// `persist_backfill(...).await?` *before* `update_sync_watermark`, so this Err
    /// is exactly what prevents the watermark from advancing past unpersisted rows
    /// — the core of the Phase-0 fix.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn persist_backfill_propagates_insert_failure() {
        let Some(pool) = test_pool().await else { return };
        let trade_repo = TradeRepo::new(pool.clone());
        let tx_repo = TransactionRepo::new(pool.clone());
        let wallet_repo = WalletRepo::new(pool.clone());

        let mint = uniq("MINT-pbfail-");
        let sig = uniq("sig-fail-");
        let mut bad = trade(&mint, &uniq("W-"), &sig, 10);
        bad.venue = "not-a-real-venue".to_string(); // violates the venue CHECK

        // Empty tx list so nothing is left behind when the trade insert fails.
        let res = persist_backfill(
            &trade_repo,
            &tx_repo,
            &wallet_repo,
            "curve",
            &[],
            std::slice::from_ref(&bad),
            &mut Vec::new(),
        )
        .await;

        assert!(
            matches!(res, Err(SyncError::Internal(_))),
            "a failed bulk insert must surface as Err (so the watermark is not stamped): {res:?}"
        );

        // Nothing was persisted for this mint.
        let saved = trade_repo.find_by_mint_all(&mint).await.expect("find");
        assert!(saved.is_empty(), "no trade rows on the failure path");

        cleanup(&pool, &mint, &[sig]).await;
    }

    /// The batched backtest fetch groups each mint's rows separately, in the same
    /// chronological order as the per-mint `find_by_mint_all`, and omits a mint
    /// with no trades — the contract the chunked backtest relies on.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_by_mints_all_groups_per_mint_in_order() {
        let Some(pool) = test_pool().await else { return };
        let trade_repo = TradeRepo::new(pool.clone());

        let mint_a = uniq("MINT-batchA-");
        let mint_b = uniq("MINT-batchB-");
        let mint_empty = uniq("MINT-batchE-");

        // Insert out of slot order so the query's ORDER BY is what sorts them.
        let sigs: Vec<String> = (0..5).map(|i| uniq(&format!("sig-batch-{i}-"))).collect();
        let rows = vec![
            trade(&mint_a, "W1", &sigs[0], 30),
            trade(&mint_a, "W1", &sigs[1], 10),
            trade(&mint_a, "W1", &sigs[2], 20),
            trade(&mint_b, "W2", &sigs[3], 15),
            trade(&mint_b, "W2", &sigs[4], 5),
        ];
        trade_repo.insert_many(&rows).await.expect("insert");

        let grouped = trade_repo
            .find_by_mints_all(&[mint_a.clone(), mint_b.clone(), mint_empty.clone()])
            .await
            .expect("batched fetch");

        // A mint with no trades is simply absent.
        assert!(!grouped.contains_key(&mint_empty), "empty mint omitted");

        // Each group matches the per-mint fetch exactly (same rows, same order).
        for mint in [&mint_a, &mint_b] {
            let single = trade_repo.find_by_mint_all(mint).await.expect("single");
            let batched = grouped.get(mint).expect("group present");
            assert_eq!(
                batched.iter().map(|t| &t.tx_signature).collect::<Vec<_>>(),
                single.iter().map(|t| &t.tx_signature).collect::<Vec<_>>(),
                "batched group equals find_by_mint_all for {mint}"
            );
        }

        cleanup(&pool, &mint_a, &sigs).await;
        cleanup(&pool, &mint_b, &[]).await;
    }
}
