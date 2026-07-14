//! Restore step 2 — backfill trades, tokens & launches from on-chain history.
//!
//! Mirrors [`DbWriter::flush`](crate::ingest::db_writer) but is driven by the RPC
//! backfill pager instead of the gRPC feed: page every restored wallet's
//! signatures, union them, then re-decode each transaction through the SAME
//! decode+map path the live feed uses (`rpc_to_protobuf` → `Decoder::decode_protobuf`
//! → `map::*`). The decoder can't tell the source apart, so the rows are identical
//! to what live ingest would have written.
//!
//! Every write is idempotent (trades dedup on `(block_time, tx_signature,
//! leg_index)`, tokens on mint, launches gated on `find_by_mint`), so a re-run adds
//! nothing. The real historical `block_time` is taken from `getTransaction.blockTime`
//! and fed in as the decoder's `received_at` — it's part of the trades dedup PK, so
//! it must be exact, not "now".

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use ingest_laserstream::backfill::{
    get_signatures_for_address, get_transactions_batch, rpc_to_protobuf, wrap_transaction_result,
};
use ingest_laserstream::decode::{DecodeOutput, Decoder};
use ingest_laserstream::event::{TokenCreated as IlTokenCreated, Trade as IlTrade};
use ingest_laserstream::{IngestEvent, Protocol};

use launcher::{variant_for_token_program, LauncherSettings};
use platform_core::models::{NewLaunch, WalletRole};
use platform_core::storage::repositories::{LaunchRepo, TokenRepo, TradeRepo, WalletDictRepo};
use platform_core::venue::{LaunchpadAdapter, MarketKind};

use super::wallets::WalletRestore;
use crate::ingest::map;
use crate::ingest::pumpfun::PumpFunAdapter;

/// Flush the buffered trade rows once they reach this many (bounds memory across a
/// large history); matches the live writer's batch shape (one `UNNEST` per flush).
const TRADE_FLUSH_EVERY: usize = 500;
/// How many signatures to fetch per `getTransaction` JSON-RPC batch (one HTTP POST).
const TX_FETCH_BATCH: usize = 100;

/// What the backfill wrote.
#[derive(Debug, Clone, Default)]
pub struct BackfillOutcome {
    /// Trade rows actually inserted (post-dedup `rows_affected`).
    pub trades: u64,
    /// Own-launch `launches` rows discovered + inserted this run.
    pub launches: usize,
    /// Every mint observed in a managed wallet's history — the reconcile set.
    pub mints: HashSet<String>,
}

/// Page every restored wallet's on-chain history and persist its trades, tokens,
/// and (for mints our dev wallets created) launches. `adapter` supplies the interned
/// `launchpad_id` / `quote_asset_id`, exactly as the live writer does.
pub async fn backfill(
    pool: &PgPool,
    settings: &LauncherSettings,
    adapter: &PumpFunAdapter,
    wallets: &WalletRestore,
) -> Result<BackfillOutcome> {
    // dev address → managed_wallet_id, for own-launch detection + linkage.
    let dev_by_address: HashMap<&str, Uuid> = wallets
        .by_address
        .iter()
        .filter(|(_, (_, role))| role == WalletRole::Dev.as_str())
        .map(|(addr, (id, _))| (addr.as_str(), *id))
        .collect();

    // Phase 2a — union every wallet's signatures (a bundle tx appears under each
    // participating wallet; dedup so we decode it once).
    let signatures = gather_signatures(settings, wallets).await?;
    info!(wallets = wallets.by_address.len(), signatures = signatures.len(), "restore backfill: gathered signatures");

    // Phase 2b — decode each tx and persist, mirroring DbWriter::flush.
    let decoder = Decoder::new(std::sync::Arc::new(Protocol::pump_fun()));
    let mut wallet_cache: HashMap<String, i32> = HashMap::new();
    let mut trade_buf: Vec<platform_core::models::NewTrade> = Vec::with_capacity(TRADE_FLUSH_EVERY);
    let mut outcome = BackfillOutcome::default();

    for chunk in signatures.chunks(TX_FETCH_BATCH) {
        let results = get_transactions_batch(&settings.rpc_url, chunk)
            .await
            .context("getTransaction batch")?;
        for (sig, maybe) in chunk.iter().zip(results) {
            let Some(raw) = maybe else { continue };
            let Some(block_time) = block_time_of(&raw) else {
                warn!(%sig, "restore backfill: tx has no blockTime — skipping (can't set trades PK)");
                continue;
            };
            let wrapped = wrap_transaction_result(sig, &raw);
            let Some(update) = rpc_to_protobuf(&wrapped) else { continue };
            let DecodeOutput::Events(events) = decoder.decode_protobuf(&update, block_time) else {
                continue;
            };
            for ev in events {
                match ev {
                    IngestEvent::Trade(t) => {
                        persist_trade(pool, adapter, &mut wallet_cache, &mut trade_buf, &t).await;
                        outcome.mints.insert(t.mint.clone());
                    }
                    IngestEvent::TokenCreated(tc) => {
                        persist_token(pool, adapter, &dev_by_address, &tc, &mut outcome).await;
                        outcome.mints.insert(tc.mint.clone());
                    }
                    // TokenMigrated / Liquidity / CreatorActivity / RawTx — not projected.
                    _ => {}
                }
            }
            if trade_buf.len() >= TRADE_FLUSH_EVERY {
                flush_trades(pool, &mut trade_buf, &mut outcome).await;
            }
        }
    }
    flush_trades(pool, &mut trade_buf, &mut outcome).await;

    info!(
        trades = outcome.trades,
        launches = outcome.launches,
        mints = outcome.mints.len(),
        "restore backfill: complete"
    );
    Ok(outcome)
}

/// Page + union every restored wallet's successful signatures (oldest-first per
/// wallet; the union is deduped, order irrelevant since each tx carries its own
/// slot/block_time). A per-wallet page failure is logged and skipped so one dead
/// address never aborts the whole restore.
async fn gather_signatures(
    settings: &LauncherSettings,
    wallets: &WalletRestore,
) -> Result<Vec<String>> {
    let mut seen: HashSet<String> = HashSet::new();
    for address in wallets.by_address.keys() {
        match get_signatures_for_address(&settings.rpc_url, address, None, None, 1000).await {
            Ok(infos) => {
                for info in infos {
                    seen.insert(info.signature);
                }
            }
            Err(e) => warn!(%address, ?e, "restore backfill: signature page failed — skipping wallet"),
        }
    }
    Ok(seen.into_iter().collect())
}

/// Map a decoded trade to a row and buffer it (interning its wallet, memoized like
/// the live writer). A sig-decode failure is logged + skipped, matching `DbWriter`.
async fn persist_trade(
    pool: &PgPool,
    adapter: &PumpFunAdapter,
    wallet_cache: &mut HashMap<String, i32>,
    trade_buf: &mut Vec<platform_core::models::NewTrade>,
    t: &IlTrade,
) {
    let wallet_ref = match wallet_ref(pool, wallet_cache, &t.wallet).await {
        Ok(id) => id,
        Err(e) => {
            warn!(?e, "restore backfill: wallet intern failed — skip trade");
            return;
        }
    };
    match map::trade_to_row(adapter, wallet_ref, t) {
        Ok(row) => trade_buf.push(row),
        Err(e) => warn!(?e, mint = %t.mint, "restore backfill: skip trade (sig decode)"),
    }
}

/// Insert a token identity row and, when a dev wallet created it, mark it an own
/// launch and (idempotently) insert its `launches` row + create signature.
async fn persist_token(
    pool: &PgPool,
    adapter: &PumpFunAdapter,
    dev_by_address: &HashMap<&str, Uuid>,
    tc: &IlTokenCreated,
    outcome: &mut BackfillOutcome,
) {
    let row = map::token_created_to_row(adapter, tc);
    if let Err(e) = TokenRepo::insert(pool, &row).await {
        warn!(?e, mint = %tc.mint, "restore backfill: token insert failed");
    }

    let Some(dev_wallet_id) = dev_by_address.get(tc.creator.as_str()).copied() else {
        return; // not one of our launches
    };

    if let Err(e) = TokenRepo::mark_own_launch(pool, &tc.mint).await {
        warn!(?e, mint = %tc.mint, "restore backfill: mark_own_launch failed");
    }

    // Gate the launch insert on absence so a re-run doesn't duplicate it.
    match LaunchRepo::find_by_mint(pool, &tc.mint).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let variant = variant_for_token_program(tc.token_program_id.as_deref());
            let new_launch = NewLaunch {
                template_id: None,
                mint_address: tc.mint.clone(),
                launchpad_id: adapter.launchpad_id(),
                variant: variant.to_string(),
                quote_asset_id: adapter.quote_asset_id(MarketKind::BondingCurve),
                dev_wallet_id: Some(dev_wallet_id),
                dev_buy_quote: None,
                status: Some("created".to_string()),
            };
            match LaunchRepo::insert(pool, &new_launch).await {
                Ok(launch) => {
                    if let Err(e) =
                        LaunchRepo::set_create_signature(pool, launch.id, &tc.signature).await
                    {
                        warn!(?e, mint = %tc.mint, "restore backfill: set_create_signature failed");
                    }
                    outcome.launches += 1;
                }
                Err(e) => warn!(?e, mint = %tc.mint, "restore backfill: launch insert failed"),
            }
        }
        Err(e) => warn!(?e, mint = %tc.mint, "restore backfill: find_by_mint failed"),
    }
}

/// Flush the buffered trades in one idempotent batch, adding the committed count to
/// the outcome. On a batch failure, retry per-row (matching `DbWriter`) so one bad
/// row doesn't drop the whole flush.
async fn flush_trades(
    pool: &PgPool,
    trade_buf: &mut Vec<platform_core::models::NewTrade>,
    outcome: &mut BackfillOutcome,
) {
    if trade_buf.is_empty() {
        return;
    }
    match TradeRepo::insert_batch(pool, trade_buf).await {
        Ok(n) => outcome.trades += n,
        Err(e) => {
            warn!(?e, count = trade_buf.len(), "restore backfill: trade batch failed — retrying per-row");
            for row in trade_buf.iter() {
                match TradeRepo::insert_batch(pool, std::slice::from_ref(row)).await {
                    Ok(n) => outcome.trades += n,
                    Err(e) => warn!(?e, "restore backfill: per-row trade insert failed — dropping"),
                }
            }
        }
    }
    trade_buf.clear();
}

/// Intern a wallet address → `wallet_dict` id, memoized (no round-trip on a hit) —
/// the same cache shape as the live `DbWriter`.
async fn wallet_ref(
    pool: &PgPool,
    cache: &mut HashMap<String, i32>,
    address: &str,
) -> Result<i32> {
    if let Some(id) = cache.get(address) {
        return Ok(*id);
    }
    let id = WalletDictRepo::intern(pool, address).await?;
    cache.insert(address.to_string(), id);
    Ok(id)
}

/// The tx's on-chain `blockTime` (Unix seconds) as a UTC timestamp, or `None` when
/// absent/not-yet-finalized. It's part of the trades dedup PK, so a tx without it
/// can't be persisted correctly and is skipped by the caller.
fn block_time_of(raw: &Value) -> Option<DateTime<Utc>> {
    let secs = raw.get("blockTime").and_then(Value::as_i64)?;
    DateTime::<Utc>::from_timestamp(secs, 0)
}
