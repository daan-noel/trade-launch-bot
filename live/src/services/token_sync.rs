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

pub use trading_core::services::pda::derive_pump_swap_pool;

use std::sync::Arc as StdArc;

use ingest_laserstream::{
    backfill::rpc_to_protobuf,
    decode::{DecodeOutput, HeliusDecoder},
    event::{IngestEvent, Side, Venue},
    proto::geyser::SubscribeUpdateTransaction,
    raw_tx::encode_payload,
    slot_anchor::SlotAnchor,
    Protocol,
};

use crate::state::{
    token_cache::{CachedTrade, TokenCache, TokenState},
    token_metrics::{self, metrics_from_state},
};
use trading_core::models::{
    raw_tx::RawTx,
    token::Token,
    trade::{Trade, TradeType},
};
use trading_core::storage::repositories::{
    raw_tx_repo::RawTxRepo,
    token_info_repo::TokenInfoRepo,
    token_repo::TokenRepo,
    trade_repo::TradeRepo,
    wallet_repo::WalletRepo,
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

/// Decoded backfill rows (raw txs + trades) buffered before a `persist_backfill`
/// flush during a streamed full ("Fetch All") backfill. Bounds the heavy raw-tx
/// frames held in memory: without it a full backfill of a high-volume migrated
/// mint accumulated its entire history at once, and several concurrent backfills
/// spiked the live process's RAM. Each flush drops the frames it persisted.
const FLUSH_BACKFILL_ROWS: usize = 5_000;

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
    /// Pinned (slot, time) anchor for estimating `block_time` on replayed frames.
    /// `None` until the first `getBlockTime` RPC call succeeds at startup.
    pub slot_anchor: Option<SlotAnchor>,
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

    // Seed a {pool → mint} index so post-migration PumpSwap (AMM) swaps — which
    // carry the pool, not the base mint — resolve back to this mint, letting both
    // the curve and AMM loops share the single `decode_protobuf` path (AMM frames
    // route to `decode_amm_live_pb` via this index). Harmless for curve txs.
    let pool_index = Arc::new(DashMap::new());
    if let Ok(pool) = derive_pump_swap_pool(&mint, &ctx.pump_program_id) {
        pool_index.insert(pool, mint.clone());
    }
    let decoder = HeliusDecoder::new(StdArc::new(Protocol::pump_fun())).with_pool_index(pool_index.clone());

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

    // Repos + decode accumulators shared by the full (streamed) and incremental
    // decode paths. Declared up front so the full path can decode + flush per gTFA
    // page below; the incremental path decodes its (dedup-bounded) batch later.
    let token_repo = TokenRepo::new(ctx.db.clone());
    let trade_repo = TradeRepo::new(ctx.db.clone());
    let tx_repo = RawTxRepo::new(ctx.db.clone());
    let wallet_repo = WalletRepo::new(ctx.db.clone());

    let mut migrate_slot: Option<u64> = None;
    let mut token_record: Option<Token> = token_repo
        .find_by_mint(&mint)
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let mut curve_txs: Vec<RawTx> = Vec::new();
    let mut curve_trades: Vec<Trade> = Vec::new();
    let mut curve_wallets: Vec<String> = Vec::new();
    let mut processed: usize = 0;

    // Full ("Fetch All") backfills use the archival getTransactionsForAddress
    // (one cursor-paginated call returning full txs, ~0.1 credit/tx). Incremental
    // ("Fetch New") uses the signatures + dedup + batched-getTransaction path: it
    // only downloads the few genuinely-new txs, cheaper than paying gTFA's per-tx
    // rate over a whole range that live ingest mostly already saved.
    let (fetched, newest_curve_sig, newest_curve_slot): (Vec<FetchedTx>, _, _) = if !req
        .incremental
    {
        // Stream gTFA pages: decode + flush every `FLUSH_BACKFILL_ROWS` so the
        // heavy raw-tx frames never accumulate over the whole (possibly huge)
        // history. gTFA returns slot-ascending pages, so the migrate-slot gate
        // still sees a migration before any later-slot trade. Returns an empty
        // `fetched` — the work is already decoded + (mostly) persisted here; the
        // shared final flush below writes the remainder.
        send_progress(
            &progress_tx,
            "processing",
            0,
            0,
            "Decoding and saving transactions",
        )
        .await?;
        let mut cursor: Option<String> = None;
        let mut newest_sig: Option<String> = None;
        let mut newest_slot: Option<i64> = None;
        loop {
            let (page, next) = gtfa_fetch_page(&rpc, &bonding_curve, cursor.as_deref()).await?;
            if let Some((sig, slot)) = page_watermark(&page) {
                newest_sig = Some(sig);
                newest_slot = Some(slot as i64);
            }
            decode_curve_batch(
                &decoder,
                &page,
                &mint,
                &req,
                &token_repo,
                &info_repo,
                &mut migrate_slot,
                &mut token_record,
                &mut curve_txs,
                &mut curve_trades,
                &mut curve_wallets,
                &progress_tx,
                &mut processed,
                0,
            )
            .await?;
            if curve_txs.len() + curve_trades.len() >= FLUSH_BACKFILL_ROWS {
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
                curve_txs.clear();
                curve_trades.clear();
                curve_wallets.clear();
            }
            match next {
                Some(t) => cursor = Some(t),
                None => break,
            }
        }
        (
            Vec::new(),
            newest_sig.or_else(|| prev_curve_sig.clone()),
            newest_slot.or(prev_curve_slot),
        )
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

    // Incremental batch: dedup-bounded, so decode it in one shot. (Empty for the
    // full path, which already streamed + flushed per page above.)
    if !fetched.is_empty() {
        send_progress(
            &progress_tx,
            "processing",
            0,
            fetched.len() as u64,
            "Decoding and saving transactions",
        )
        .await?;
        decode_curve_batch(
            &decoder,
            &fetched,
            &mint,
            &req,
            &token_repo,
            &info_repo,
            &mut migrate_slot,
            &mut token_record,
            &mut curve_txs,
            &mut curve_trades,
            &mut curve_wallets,
            &progress_tx,
            &mut processed,
            fetched.len() as u64,
        )
        .await?;
    }

    // Final flush: the full path's leftover (< FLUSH_BACKFILL_ROWS) remainder, or
    // the whole incremental batch. `persist_backfill` propagates any write failure
    // and the sync watermark is stamped only on overall success (below), so a
    // mid-stream flush failure never advances the boundary past unsaved rows.
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
    // `trades` (full `Trade`) is also returned uncapped in `SyncOutput` below,
    // while `recompute_token_state` rebuilds `state.trades` through the 50K
    // retention ring — so the two are not interchangeable for high-volume mints.
    // Project to the slim `CachedTrade` for the cache window, interning each wallet
    // into the token's `u32` namespace (Phase B step 2); the uncapped `Trade` vec is
    // retained for the API output. `recompute_token_state` below carries this
    // interner forward, so the retained ids stay valid. Cold sync path.
    let cached: Vec<CachedTrade> = trades.iter().map(|t| state.intern_trade(t)).collect();
    state.trades = std::sync::Arc::new(cached);
    state.is_migrated = is_migrated;

    token_metrics::recompute_token_state(&mut state);

    // `metrics_from_state` folds in the cheap in-memory dead-token verdict.
    let metrics = metrics_from_state(&mint, &state);
    write_metrics(&info_repo, &metrics).await?;

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
    // frames carry no on-chain blockTime. Use the SlotAnchor to estimate the
    // real block_time from each frame's slot; fall back to now() when no anchor
    // is pinned yet (first sync before startup RPC call completed).
    let anchor = ctx.slot_anchor;
    let mut newest_sig: Option<String> = None;
    let mut newest_slot: Option<u64> = None;
    let mut txs: Vec<FetchedTx> = Vec::with_capacity(replayed.len());
    for r in replayed {
        if newest_slot.map_or(true, |s| r.slot >= s) {
            newest_slot = Some(r.slot);
            newest_sig = update_signature(&r.update);
        }
        let block_time = anchor
            .map(|a| a.estimate_block_time(r.slot))
            .unwrap_or_else(Utc::now);
        txs.push(FetchedTx {
            slot: r.slot,
            block_time,
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
    tx_repo: &RawTxRepo,
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

    // Decoded AMM rows persist via `persist_backfill`, which propagates any write
    // failure; the caller stamps the AMM watermark only on `Ok`, so a failed write
    // (or a mid-stream flush failure on the full path) can't advance the boundary
    // past unsaved swaps — the next incremental sync re-pulls them.
    // `decode_amm_protobuf` resolves each PumpSwap swap's pool to `mint` via the
    // decoder's seeded {pool → mint} index, dropping swaps for any other pool. It
    // has no curve-priority gate (unlike `decode_protobuf`), so an aggregator tx
    // that also touches the curve program still yields our pool's AMM trade.
    let mut amm_txs: Vec<RawTx> = Vec::new();
    let mut amm_trades: Vec<Trade> = Vec::new();
    let mut amm_wallets: Vec<String> = Vec::new();

    // Full backfill streams archival gTFA pages, decoding + flushing every
    // `FLUSH_BACKFILL_ROWS` so the raw-tx frames don't accumulate over the whole
    // pool history (the curve path's streaming, applied to the AMM venue).
    let (newest_amm_sig, newest_amm_slot) = if !incremental {
        send_progress(progress_tx, "processing", 0, 0, "Decoding AMM trades").await?;
        let mut cursor: Option<String> = None;
        let mut newest_sig: Option<String> = None;
        let mut newest_slot: Option<i64> = None;
        loop {
            let (page, next) = gtfa_fetch_page(rpc, &pool, cursor.as_deref()).await?;
            if let Some((sig, slot)) = page_watermark(&page) {
                newest_sig = Some(sig);
                newest_slot = Some(slot as i64);
            }
            decode_amm_batch(decoder, &page, &mut amm_txs, &mut amm_trades, &mut amm_wallets);
            if amm_txs.len() + amm_trades.len() >= FLUSH_BACKFILL_ROWS {
                persist_backfill(
                    trade_repo, tx_repo, wallet_repo, "amm", &amm_txs, &amm_trades,
                    &mut amm_wallets,
                )
                .await?;
                amm_txs.clear();
                amm_trades.clear();
                amm_wallets.clear();
            }
            match next {
                Some(t) => cursor = Some(t),
                None => break,
            }
        }
        (newest_sig, newest_slot)
    } else {
        // Incremental tries the LaserStream replay window first, then falls back to
        // the signatures + dedup path. Both are dedup-bounded, so decode in one shot.
        let (fetched, newest_amm_sig, newest_amm_slot) = if let Some((txs, sig, slot)) =
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

            // Skip AMM swaps already saved (e.g. by live ingest) so we don't
            // re-fetch them from Helius.
            let candidates: Vec<String> =
                signatures.iter().map(|e| e.signature.clone()).collect();
            let saved = trade_repo
                .saved_signatures(mint, "amm", &candidates)
                .await
                .map_err(|e| SyncError::Internal(e.to_string()))?;
            let to_fetch = signatures
                .into_iter()
                .filter(|e| !saved.contains(&e.signature))
                .collect::<Vec<SignatureEntry>>();

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
        decode_amm_batch(decoder, &fetched, &mut amm_txs, &mut amm_trades, &mut amm_wallets);
        (newest_amm_sig, newest_amm_slot)
    };

    // Final flush: full path's remainder, or the whole incremental batch.
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
/// is sorted/deduped in place before the single bulk touch. Raw txs carry
/// `source=1` (sync — both "Fetch All" via gTFA and "Fetch New" via LaserStream
/// replay), stamped by `persist_tx`; the live ingest pipeline writes `source=0`.
async fn persist_backfill(
    trade_repo: &TradeRepo,
    tx_repo: &RawTxRepo,
    wallet_repo: &WalletRepo,
    venue: &str,
    txs: &[RawTx],
    trades: &[Trade],
    wallets: &mut Vec<String>,
) -> Result<(), SyncError> {
    tx_repo
        .insert_many(txs)
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

/// Stamp a proxy `tx_index` on RPC-lowered backfill frames (in-place).
///
/// `rpc_to_protobuf` can't recover a transaction's real position within its block —
/// single-tx `getTransaction` / gTFA pages don't carry it — so every lowered frame
/// has `info.index == 0`. Live ingest and LaserStream-replay frames keep their real
/// feed index; only these RPC frames need a fill, hence stamping here (not in the
/// shared decoder). We assign a per-slot running counter so the trades sort by a
/// stable `(slot, tx_index, leg_index)` key. It is NOT the absolute block position —
/// only a monotonic-within-slot proxy — so never join/compare it against live
/// `tx_index`.
///
/// **Within-slot direction.** gTFA's `"asc"` orders *across slots* (oldest-first),
/// but within one slot the page returns transactions **newest-first** — verified
/// against the curve reserve chain (DB: same-slot rows chain correctly under
/// `tx_index DESC`, never `ASC`, when stamped in incoming order). So we assign the
/// counter **in reverse within each slot**: the last frame of a slot run gets index
/// 0, the first gets the highest. That makes `tx_index ASC` match true on-chain
/// execution order, so every downstream consumer (swing scan, chart, sweep) can
/// trust `slot ASC → tx_index ASC → leg_index ASC` with no reserve-chain
/// reconstruction.
///
/// `frames` MUST be slot-ascending (every caller sorts first). The counter is
/// per-call, so a single slot split across two streamed gTFA pages restarts —
/// negligible for one account's history, and at worst reorders that one boundary
/// slot's intra-slot ties.
fn stamp_proxy_tx_index(frames: &mut [FetchedTx]) {
    let mut i = 0;
    while i < frames.len() {
        // Find the run of frames sharing this slot (frames are slot-ascending).
        let slot = frames[i].slot;
        let mut j = i + 1;
        while j < frames.len() && frames[j].slot == slot {
            j += 1;
        }
        // gTFA returns the slot's txs newest-first, so reverse: the run's LAST
        // frame is the slot's first on-chain tx (index 0).
        let run_len = (j - i) as u64;
        for (k, ft) in frames[i..j].iter_mut().enumerate() {
            if let Some(info) = ft.update.transaction.as_mut() {
                info.index = run_len - 1 - k as u64;
            }
        }
        i = j;
    }
}

/// Base58 signature of a lowered frame (for the sync watermark).
fn update_signature(update: &SubscribeUpdateTransaction) -> Option<String> {
    update
        .transaction
        .as_ref()
        .map(|i| bs58::encode(&i.signature).into_string())
}

/// Build the persisted [`RawTx`] for a decoded backfill tx (`source=1`, sync).
///
/// The `payload` is the verbatim protobuf wire bytes via the shared
/// [`encode_payload`] — the same byte format the live gRPC path persists, so
/// live and historical `raw_txs` rows are byte-consistent for later replay.
/// `block_time` is the real on-chain time from the carrier; `tx_index` is the
/// frame's block position (a slot-ordered proxy on RPC-lowered frames, see
/// [`stamp_proxy_tx_index`]).
fn persist_tx(update: &SubscribeUpdateTransaction, slot: u64, block_time: DateTime<Utc>) -> RawTx {
    let info = update.transaction.as_ref();
    let signature = info.map(|i| i.signature.clone()).unwrap_or_default();
    let tx_index = info.map(|i| i.index as i32).unwrap_or(0);
    RawTx::new(signature, slot as i64, block_time, tx_index, encode_payload(update), 1)
}

/// Fetch ONE archival `getTransactionsForAddress` (gTFA) page for `address`,
/// cursor-paginated. Each item is already shaped like a `getTransaction` result,
/// so we lower it to the decoder frame directly — no second round-trip per
/// signature. Returns the page (slot-ascending) plus the next cursor (`None` ⇒
/// end of history).
///
/// The full ("Fetch All") backfill streams these pages, decoding + flushing each
/// so the whole history never materializes at once. ~0.1 credit/tx at a 1000-tx
/// page vs 1 credit/tx for per-sig `getTransaction`. Like the rpc "Fetch All"
/// path it returns every tx, so decoder fixes propagate via the trades upsert.
async fn gtfa_fetch_page(
    rpc: &HeliusRpc,
    address: &str,
    cursor: Option<&str>,
) -> Result<(Vec<FetchedTx>, Option<String>), SyncError> {
    // Oldest-first so pages are slot-ascending across the whole history (the
    // migrate-slot gate relies on it). `base64` so each item lowers to protobuf
    // via `rpc_to_protobuf` (see `fetched_from_rpc`).
    let (data, next) = rpc
        .get_transactions_for_address_full_page_enc(address, "asc", GTFA_PAGE_LIMIT, cursor, "base64")
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    let mut page: Vec<FetchedTx> = Vec::with_capacity(data.len());
    for item in &data {
        let slot = item.get("slot").and_then(|v| v.as_u64()).unwrap_or(0);
        // base64 items expose no `transaction.signatures`; pass "" and let the
        // adapter recover the signature from the bincode tx.
        if let Some(ft) = fetched_from_rpc(slot, &wrap_transaction_result("", item)) {
            page.push(ft);
        }
    }
    // Guarantee in-page slot order (the gate + cross-page watermark assume it).
    page.sort_by_key(|t| t.slot);
    // RPC frames carry no block position; fill a slot-ordered proxy tx_index.
    stamp_proxy_tx_index(&mut page);
    Ok((page, next))
}

/// Newest `(signature, slot)` in a slot-ascending page — the highest-slot frame
/// that carries a signature. Used to advance the sync watermark per page during a
/// streamed backfill (a later non-empty page always has ≥ slots, so overwriting
/// is correct; an empty/sig-less page leaves the prior watermark untouched).
fn page_watermark(page: &[FetchedTx]) -> Option<(String, u64)> {
    page.iter()
        .rev()
        .find_map(|ft| update_signature(&ft.update).map(|sig| (sig, ft.slot)))
}

/// Decode + accumulate one curve batch (a gTFA page or the incremental batch)
/// into the shared backfill buffers. Mutating in place lets the full path flush
/// the buffers between pages while keeping the migrate-slot gate (which needs a
/// migration seen earlier in slot order) and the token-creation upsert inline.
/// `processed`/`total_hint` drive the "Processed N" progress (`total_hint` is 0
/// for the streamed full path, where the total is unknown until the last page).
#[allow(clippy::too_many_arguments)]
async fn decode_curve_batch(
    decoder: &HeliusDecoder,
    batch: &[FetchedTx],
    mint: &str,
    req: &TokenSyncRequest,
    token_repo: &TokenRepo,
    info_repo: &TokenInfoRepo,
    migrate_slot: &mut Option<u64>,
    token_record: &mut Option<Token>,
    txs: &mut Vec<RawTx>,
    trades: &mut Vec<Trade>,
    wallets: &mut Vec<String>,
    progress_tx: &mpsc::Sender<String>,
    processed: &mut usize,
    total_hint: u64,
) -> Result<(), SyncError> {
    for entry in batch {
        *processed += 1;
        if processed.is_multiple_of(25) {
            send_progress(
                progress_tx,
                "processing",
                *processed as u64,
                total_hint,
                &format!("Processed {processed}"),
            )
            .await?;
        }

        let slot = entry.slot;
        let skip_trades = !req.include_post_migrate && migrate_slot.is_some_and(|ms| slot > ms);

        if let DecodeOutput::Events(mut events) =
            decoder.decode_protobuf(&entry.update, entry.block_time)
        {
            sort_sync_events(&mut events);

            txs.push(persist_tx(&entry.update, entry.slot, entry.block_time));

            for event in events {
                match event {
                    IngestEvent::TokenCreated(e) if e.mint == mint => {
                        let token = token_from_ingest_event(e);
                        token_repo
                            .upsert(&token)
                            .await
                            .map_err(|e| SyncError::Internal(e.to_string()))?;
                        wallets.push(token.creator_wallet.clone());
                        *token_record = Some(token);
                    }
                    IngestEvent::Trade(e) if e.mint == mint => {
                        if skip_trades {
                            continue;
                        }
                        let mut trade = trade_from_ingest_event(&e);
                        trade.received_at = Utc::now();
                        wallets.push(trade.wallet_address.clone());
                        trades.push(trade);
                    }
                    IngestEvent::TokenMigrated(e) if e.mint == mint => {
                        *migrate_slot = Some(e.slot);
                        if let Err(err) = info_repo.update_migration_status(mint, true).await {
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
    Ok(())
}

/// Decode + accumulate one AMM batch (a gTFA page or the incremental batch) into
/// the shared backfill buffers. No migrate gate / token upsert (unlike the curve
/// path), so it's synchronous.
fn decode_amm_batch(
    decoder: &HeliusDecoder,
    batch: &[FetchedTx],
    txs: &mut Vec<RawTx>,
    trades: &mut Vec<Trade>,
    wallets: &mut Vec<String>,
) {
    for entry in batch {
        if let DecodeOutput::Events(events) =
            decoder.decode_amm_protobuf(&entry.update, entry.block_time)
        {
            txs.push(persist_tx(&entry.update, entry.slot, entry.block_time));
            for event in events {
                if let IngestEvent::Trade(e) = event {
                    let mut trade = trade_from_ingest_event(&e);
                    trade.received_at = Utc::now();
                    wallets.push(trade.wallet_address.clone());
                    trades.push(trade);
                }
            }
        }
    }
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
    // RPC frames carry no block position; fill a slot-ordered proxy tx_index.
    stamp_proxy_tx_index(&mut out);
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

fn sort_sync_events(events: &mut [IngestEvent]) {
    events.sort_by_key(|e| match e {
        IngestEvent::TokenCreated(_) => 0,
        IngestEvent::TokenMigrated(_) => 1,
        IngestEvent::Trade(_) => 2,
        IngestEvent::CreatorActivity(_) => 3,
        IngestEvent::Liquidity(_) => 4,
        IngestEvent::RawTx(_) => 5,
    });
}

async fn write_metrics(
    info_repo: &TokenInfoRepo,
    m: &crate::state::token_metrics::TokenMetricsWrite,
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
            m.is_dead,
            m.is_migrated,
            m.lifetime_secs,
        )
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))
}

// ── IngestEvent → trading_core type translators ───────────────────────────────

fn trade_from_ingest_event(e: &ingest_laserstream::event::Trade) -> Trade {
    use uuid::Uuid;
    Trade {
        id: Uuid::new_v4(),
        mint_address: e.mint.clone(),
        wallet_address: e.wallet.clone(),
        trade_type: match e.side {
            Side::Buy => TradeType::Buy,
            Side::Sell => TradeType::Sell,
        },
        sol_amount: e.sol,
        token_amount: e.tokens,
        price_per_token: e.price,
        tx_signature: e.signature.clone(),
        tx_index: e.tx_index,
        leg_index: e.leg_index,
        slot: e.slot,
        block_time: e.block_time,
        received_at: e.received_at,
        reserve_sol: e.reserves.virtual_sol,
        reserve_token: e.reserves.virtual_token,
        real_sol_reserves: e.reserves.real_sol,
        real_token_reserves: e.reserves.real_token,
        instruction_type: e.instruction_type.clone(),
        instruction_labels: serde_json::json!(e.instruction_labels),
        venue: match e.venue { Venue::Curve => "curve", Venue::Amm => "amm" }.to_string(),
    }
}

fn token_from_ingest_event(e: ingest_laserstream::event::TokenCreated) -> Token {
    use uuid::Uuid;
    Token {
        id: Uuid::new_v4(),
        mint_address: e.mint,
        creator_wallet: e.creator,
        name: e.name,
        symbol: e.symbol,
        token_program_id: e.token_program_id,
        bonding_curve_address: e.bonding_curve,
        initial_supply_token: e.initial_supply,
        initial_buy_sol: e.initial_buy_sol,
        initial_buy_instruction: e.initial_buy_instruction.as_ref().map(|_| serde_json::Value::Null),
        cu_limit: e.cu_limit,
        cu_price: e.cu_price,
        is_mayhem_mode: e.is_mayhem_mode,
        is_cashback_enabled: e.is_cashback_enabled,
        instruction_labels: serde_json::json!(e.instruction_labels),
        creation_tx_signature: e.signature,
        created_at: e.block_time,
    }
}

#[cfg(test)]
mod amm_verification {
    //! On-chain verification of PumpSwap pool derivation + event decoding.
    //!
    //! Ignored by default (needs network + a Helius key). Run with:
    //!   HELIUS_RPC_URL="<url>" cargo test -p backend amm_pool_derivation -- --ignored --nocapture
    use super::*;
    use trading_core::config::constants::{PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID, WSOL_MINT};
    use ingest_laserstream::decode::HeliusDecoder;
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
        // Diagnostic buckets for canonical swaps that decode to zero trades — to
        // tell apart a benign filter (dust) from a real shared-decoder gap (an
        // event the Borsh layout can't read, which would hit LIVE ingest too).
        let (mut empty_no_event, mut empty_dust, mut empty_short, mut empty_unknown) =
            (0u32, 0u32, 0u32, 0u32);

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
                // A pump.fun mint that doesn't match canonical derivation *could*
                // be a real bug — but only flag it after confirming the event pool
                // genuinely belongs to THIS base mint as a canonical (index 0,
                // WSOL-quote) PumpSwap pool. A routed/aggregator tx swaps several
                // legs, so the "first non-WSOL mint" (base_mint) and the "first swap
                // event pool" (event_pool) can come from *different* legs — e.g. this
                // base mint paired with another leg's USDC pool — which is a
                // cross-attribution artifact of the sampling, not a derivation bug.
                if base_mint.ends_with("pump")
                    && pool_is_canonical_wsol_for(&client, &url, &event_pool, &base_mint).await
                {
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
                HeliusDecoder::new(StdArc::new(Protocol::pump_fun())).with_pool_index(idx);
            let update = ingest_laserstream::backfill::rpc_to_protobuf(
                &wrap_transaction_result(sig, &tx_b64),
            )
            .expect("canonical swap must lower to protobuf");
            let trades: Vec<_> = match amm_decoder.decode_amm_protobuf(&update, Utc::now()) {
                DecodeOutput::Events(events) => events
                    .into_iter()
                    .filter_map(|e| match e {
                        IngestEvent::Trade(t) => Some(trade_from_ingest_event(&t)),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            if trades.is_empty() {
                // Canonical pool derivation already matched (the point of THIS test);
                // decoding the full tx is a secondary smoke check. Classify WHY this
                // canonical swap yielded no trade, so the skip is attributed to a
                // concrete cause rather than waved off. The AMM decode is purely
                // log-driven (`decode_pump_swap_trades_from_logs`) and SHARED with
                // live ingest, so anything but "dust"/"no-event-in-logs" is a real
                // shared-decoder gap. Re-scan the same `Program data:` lines with the
                // independent offset parser used above (pool @ [120..152],
                // user_quote_amount @ [112..120]) and find the event for `derived`.
                let derived_event = logs.iter().find_map(|log| {
                    let encoded = log.strip_prefix("Program data: ")?;
                    let bytes = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        encoded,
                    )
                    .ok()?;
                    if bytes.len() < 152 {
                        return None;
                    }
                    let disc = &bytes[..8];
                    let is_swap = disc == trading_core::config::constants::PUMP_SWAP_BUY_EVENT_DISCRIMINATOR
                        || disc == trading_core::config::constants::PUMP_SWAP_SELL_EVENT_DISCRIMINATOR;
                    if !is_swap || bs58::encode(&bytes[120..152]).into_string() != derived {
                        return None;
                    }
                    let quote_lamports =
                        u64::from_le_bytes(bytes[112..120].try_into().unwrap());
                    Some((bytes.len(), quote_lamports as f64 / 1e9))
                });
                match derived_event {
                    None => {
                        // No swap event for the derived pool survives in the logs at
                        // all (e.g. RPC log truncation on a deep aggregator tx). Not a
                        // decoder bug — a transport/log-availability gap.
                        empty_no_event += 1;
                        println!("EMPTY {sig} reason=no-derived-event-in-logs");
                    }
                    Some((_, quote_sol)) if trading_core::models::trade::Trade::is_dust(quote_sol) => {
                        // Below the 10k-lamport ingest dust floor — dropped on purpose,
                        // on BOTH live and backfill. Correct behavior, not a gap.
                        empty_dust += 1;
                        println!("EMPTY {sig} reason=dust quote_sol={quote_sol:.9}");
                    }
                    Some((len, quote_sol)) if len < 184 => {
                        // Event shorter than the Borsh layout reads (timestamp + 13×u64
                        // + pool + user = 184B): `RawPumpSwap*Event::deserialize` fails
                        // → the SHARED decoder drops it on live too. A real gap.
                        empty_short += 1;
                        println!(
                            "EMPTY {sig} reason=short-event len={len} quote_sol={quote_sol:.9} (Borsh fail — affects LIVE)"
                        );
                    }
                    Some((len, quote_sol)) => {
                        // Full-length, non-dust event for the right pool that the
                        // production decoder still dropped — an unexplained gap in the
                        // SHARED decoder path; would also drop on live.
                        empty_unknown += 1;
                        println!(
                            "EMPTY {sig} reason=UNKNOWN len={len} quote_sol={quote_sol:.9} (full-length non-dust — affects LIVE)"
                        );
                    }
                }
                continue;
            }
            let t = &trades[0];
            assert_eq!(t.venue, "amm");
            assert!(t.sol_amount > 0.0 && t.token_amount > 0);
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
        println!(
            "empty_decode breakdown: no_event_in_logs={empty_no_event} dust={empty_dust} short_event={empty_short} unknown={empty_unknown}  (short+unknown = shared-decoder gaps that also affect LIVE ingest)"
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
        // The AMM decode is purely log-driven and SHARED with live ingest. A
        // canonical swap that decodes to zero trades is only acceptable when it's
        // dust (intentional 10k-lamport floor) or its event was truncated out of
        // the RPC logs (a transport gap, not a decoder one). A `short_event`
        // (event the Borsh layout can't read — e.g. a PumpSwap event-layout change)
        // or an `unknown` (full-length, non-dust, right pool, still dropped) is a
        // real gap that would ALSO drop the trade on live ingest — fail loudly so a
        // future layout drift can't silently make backfill + live miss swaps.
        assert_eq!(
            (empty_short, empty_unknown),
            (0, 0),
            "shared AMM decoder dropped non-dust canonical swap(s): short_event={empty_short} unknown={empty_unknown} — affects LIVE ingest, not just backfill"
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
            if disc == trading_core::config::constants::PUMP_SWAP_BUY_EVENT_DISCRIMINATOR
                || disc == trading_core::config::constants::PUMP_SWAP_SELL_EVENT_DISCRIMINATOR
            {
                return Some(bs58::encode(&bytes[120..152]).into_string());
            }
        }
        None
    }

    /// True iff `pool` is on-chain a *canonical* PumpSwap pool for `base_mint`:
    /// owned by the PumpSwap program, index 0, WSOL quote, and its `base_mint`
    /// field equals `base_mint`. Used to reject cross-attribution false positives
    /// (a routed tx pairing one leg's base mint with another leg's pool) before
    /// treating a derivation mismatch as a real bug. Pool account layout after the
    /// 8-byte discriminator: pool_bump(1) + index(u16) + creator(32) +
    /// base_mint(32) + quote_mint(32).
    async fn pool_is_canonical_wsol_for(
        client: &reqwest::Client,
        url: &str,
        pool: &str,
        base_mint: &str,
    ) -> bool {
        let acc = rpc(
            client,
            url,
            "getAccountInfo",
            json!([pool, {"encoding":"base64","commitment":"confirmed"}]),
        )
        .await;
        if acc["value"]["owner"].as_str() != Some(PUMP_SWAP_PROGRAM_ID) {
            return false;
        }
        let Some(b64) = acc["value"]["data"][0].as_str() else {
            return false;
        };
        let Ok(data) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
        else {
            return false;
        };
        if data.len() < 107 {
            return false;
        }
        let index = u16::from_le_bytes([data[9], data[10]]);
        let pool_base = bs58::encode(&data[43..75]).into_string();
        let pool_quote = bs58::encode(&data[75..107]).into_string();
        index == 0 && pool_quote == WSOL_MINT && pool_base == base_mint
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
        use ingest_laserstream::backfill::rpc_to_protobuf;
        use base64::{engine::general_purpose::STANDARD, Engine};
        use solana_sdk::{message::VersionedMessage, transaction::VersionedTransaction};

        let url = std::env::var("HELIUS_RPC_URL").expect("set HELIUS_RPC_URL");
        let helius = HeliusRpc::new(url);
        let decoder = HeliusDecoder::new(StdArc::new(Protocol::pump_fun()));

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
            if let DecodeOutput::Events(events) =
                decoder.decode_protobuf(&update, Utc::now())
            {
                decoded += 1;
                for e in &events {
                    if let IngestEvent::Trade(t) = e {
                        assert!(
                            t.sol > 0.0 && t.tokens > 0,
                            "decoded trade has non-positive amounts: {t:?}",
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
    use trading_core::models::trade::TradeType;
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

    fn raw_tx(sig: &str, slot: u64) -> RawTx {
        // raw_txs.tx_signature is opaque BYTEA (no base58 validation), so the
        // test's string sig is stored as its bytes; payload is a small stand-in.
        RawTx::new(
            sig.as_bytes().to_vec(),
            slot as i64,
            Utc::now(),
            0,
            sig.as_bytes().to_vec(),
            1,
        )
    }

    fn trade(mint: &str, wallet: &str, sig: &str, slot: u64) -> Trade {
        Trade::new(
            mint.to_string(),
            wallet.to_string(),
            TradeType::Buy,
            0.5,
            1_000,
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
        let sig_bytes: Vec<Vec<u8>> = sigs.iter().map(|s| s.as_bytes().to_vec()).collect();
        let _ = sqlx::query("DELETE FROM raw_txs WHERE tx_signature = ANY($1)")
            .bind(&sig_bytes)
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
        let tx_repo = RawTxRepo::new(pool.clone());
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
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM raw_txs WHERE tx_signature=$1)")
                    .bind(sig.as_bytes())
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
        let tx_repo = RawTxRepo::new(pool.clone());
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
