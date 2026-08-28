//! Restore step 2 — backfill trades, tokens & launches from on-chain history.
//!
//! Mirrors [`DbWriter::flush`](crate::ingest::db_writer) but is driven by the RPC
//! backfill pager instead of the gRPC feed: stream every restored wallet's history
//! through the **archival** `getTransactionsForAddress` (gTFA) pager and re-decode
//! each transaction through the SAME decode+map path the live feed uses
//! (`rpc_to_protobuf` → `Decoder::decode_protobuf` → `map::*`). The decoder can't
//! tell the source apart, so the rows are identical to what live ingest would have
//! written.
//!
//! gTFA returns each transaction already `getTransaction`-shaped, so it collapses
//! the old `getSignaturesForAddress` + N×`getTransaction` loop into one
//! cursor-paginated call at ~1/10th the credit cost (0.1 vs 1 credit/tx). A bundle
//! tx shows up under every participating wallet, so gTFA hands it back once per
//! wallet — a run-level signature set decodes+persists it only once (writes are
//! idempotent regardless).
//!
//! Every write is idempotent (trades dedup on `(block_time, tx_signature,
//! leg_index)`, tokens on mint, launches gated on `find_by_mint`), so a re-run adds
//! nothing. The real historical `block_time` is taken from `getTransaction.blockTime`
//! and fed in as the decoder's `received_at` — it's part of the trades dedup PK, so
//! it must be exact, not "now".

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use ingest_pumpfun::backfill::{
    get_transactions_for_address_page, rpc_to_protobuf, wrap_transaction_result,
};
use ingest_pumpfun::decode::{DecodeOutput, Decoder};
use ingest_pumpfun::event::{TokenCreated as IlTokenCreated, Trade as IlTrade};
use ingest_pumpfun::{IngestEvent, Protocol};

use launcher::{variant_for_token_program, LauncherSettings};
use platform_core::models::{NewLaunch, WalletRole};
use platform_core::storage::repositories::{LaunchRepo, TokenRepo, TradeRepo, WalletDictRepo};
use platform_core::venue::{LaunchpadAdapter, MarketKind};

use super::wallets::WalletRestore;
use crate::ingest::map;
use crate::ingest::pumpfun::PumpFunAdapter;
use crate::sse::SseHub;

/// Flush the buffered trade rows once they reach this many (bounds memory across a
/// large history); matches the live writer's batch shape (one `UNNEST` per flush).
const TRADE_FLUSH_EVERY: usize = 500;
/// Transactions per archival `getTransactionsForAddress` (gTFA) page — the RPC max,
/// where the archival method is cheapest (~0.1 credit/tx = 10 credits / 100 txs).
const GTFA_PAGE_LIMIT: usize = 1000;

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
    sse: &SseHub,
) -> Result<BackfillOutcome> {
    // dev address → managed_wallet_id, for own-launch detection + linkage.
    let dev_by_address: HashMap<&str, Uuid> = wallets
        .by_address
        .iter()
        .filter(|(_, (_, role))| role == WalletRole::Dev.as_str())
        .map(|(addr, (id, _))| (addr.as_str(), *id))
        .collect();

    // Decode each tx and persist, mirroring DbWriter::flush. gTFA hands a shared
    // bundle tx back once per participating wallet, so `seen_sigs` decodes+persists
    // it once (idempotent writes make this an optimization, not a correctness need).
    let decoder = Decoder::new(std::sync::Arc::new(Protocol::pump_fun()));
    let mut wallet_cache: HashMap<String, i32> = HashMap::new();
    let mut trade_buf: Vec<platform_core::models::NewTrade> = Vec::with_capacity(TRADE_FLUSH_EVERY);
    let mut seen_sigs: HashSet<String> = HashSet::new();
    let mut outcome = BackfillOutcome::default();

    // Streamed total is unknown up front (no signature pre-pass to bill), so report
    // a growing processed count rather than a precise ratio.
    let mut processed = 0usize;
    sse.restore_progress("backfill", 0, 0);
    for address in wallets.by_address.keys() {
        let mut cursor: Option<String> = None;
        loop {
            let (page, next) = match get_transactions_for_address_page(
                &settings.rpc_url,
                address,
                "asc",
                GTFA_PAGE_LIMIT,
                cursor.as_deref(),
                "base64",
            )
            .await
            {
                Ok(page) => page,
                // One dead/rate-limited wallet must never abort the whole restore.
                Err(e) => {
                    warn!(%address, ?e, "restore backfill: gTFA page failed — skipping rest of wallet");
                    break;
                }
            };
            if page.is_empty() {
                break;
            }

            for raw in &page {
                let Some(block_time) = block_time_of(raw) else {
                    // No blockTime ⇒ can't set the trades PK; skip (matches the old path).
                    continue;
                };
                // base64 gTFA items expose no signature field; pass "" and let the
                // adapter recover it from the bincode tx.
                let wrapped = wrap_transaction_result("", raw);
                let Some(update) = rpc_to_protobuf(&wrapped) else { continue };
                let DecodeOutput::Events(events) = decoder.decode_protobuf(&update, block_time)
                else {
                    continue;
                };

                // Dedup at the tx level: a tx's projected events share one signature,
                // so skip the whole tx if another wallet's page already handled it.
                if let Some(sig) = events.iter().find_map(event_signature) {
                    if !seen_sigs.insert(sig) {
                        continue;
                    }
                }

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

            processed += page.len();
            sse.restore_progress("backfill", processed, processed);

            match next {
                Some(token) => cursor = Some(token),
                None => break,
            }
        }
    }
    flush_trades(pool, &mut trade_buf, &mut outcome).await;

    info!(
        wallets = wallets.by_address.len(),
        processed,
        unique_txs = seen_sigs.len(),
        trades = outcome.trades,
        launches = outcome.launches,
        mints = outcome.mints.len(),
        "restore backfill: complete"
    );
    Ok(outcome)
}

/// A tx's base58 signature from its first projected (trade/create) event — every
/// event decoded from one tx carries the same signature, so any one identifies the
/// tx for the run-level dedup. `None` for a tx with no projected event (nothing to
/// persist, so its dedup is moot anyway).
fn event_signature(ev: &IngestEvent) -> Option<String> {
    match ev {
        IngestEvent::Trade(t) => Some(t.signature.clone()),
        IngestEvent::TokenCreated(tc) => Some(tc.signature.clone()),
        _ => None,
    }
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
/// can't be persisted correctly and is skipped by the caller. `pub(crate)` so the
/// offline fixture test replays the same block_time the backfill would.
pub(crate) fn block_time_of(raw: &Value) -> Option<DateTime<Utc>> {
    let secs = raw.get("blockTime").and_then(Value::as_i64)?;
    DateTime::<Utc>::from_timestamp(secs, 0)
}

