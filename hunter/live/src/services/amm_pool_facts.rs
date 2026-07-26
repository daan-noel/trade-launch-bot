//! Durable AMM pool-facts bridge: persist the trader's harvested PumpSwap pool
//! layout to Postgres and re-seed it on boot, so a restart never has to rebuild
//! the swap tail via RPC for a held migrated token whose pool has since gone dead.
//!
//! The facts live in the executor's in-memory cache (warmed for free by the feed
//! harvest, `observe_amm_swap_accounts`, zero RPC). This module — in `live`, the
//! only crate that depends on BOTH the executor and the DB — carries them across
//! the boundary the executor (a standalone drop-in) can't cross itself:
//!   - [`spawn_persist_loop`]: background task upserting newly cached pools.
//!   - [`seed_from_db`]: boot-time re-seed of the trader cache for held mints.
//!
//! Neither path touches the hot ingest/exit loops.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use pump_trader::{AmmPoolFacts, PumpFunTrader};
use sqlx::PgPool;
use tracing::{debug, error, info};
use trading_core::storage::repositories::amm_pool_facts_repo::{AmmPoolFactRow, AmmPoolFactsRepo};

/// How often the persist loop scans the trader cache for newly learned pools. A
/// new pool appears only when a migrated token is first traded/harvested, so a
/// slow cadence is ample; this is not a hot path.
const PERSIST_INTERVAL: Duration = Duration::from_secs(45);

/// Executor DTO → DB row (adds the mint key). Keep in lockstep with
/// `AmmPoolFacts` / `AmmPoolFactRow` — both mirror the on-chain pool layout.
fn to_row(mint: &str, f: &AmmPoolFacts) -> AmmPoolFactRow {
    AmmPoolFactRow {
        mint_address: mint.to_string(),
        pool: f.pool.clone(),
        base_mint: f.base_mint.clone(),
        quote_mint: f.quote_mint.clone(),
        base_token_program: f.base_token_program.clone(),
        pool_base_token_account: f.pool_base_token_account.clone(),
        pool_quote_token_account: f.pool_quote_token_account.clone(),
        coin_creator: f.coin_creator.clone(),
        coin_creator_vault_ata: f.coin_creator_vault_ata.clone(),
        coin_creator_vault_authority: f.coin_creator_vault_authority.clone(),
        is_cashback_coin: f.is_cashback_coin,
        fee_share_marker: f.fee_share_marker.clone(),
        needs_pool_v2: f.needs_pool_v2,
    }
}

/// DB row → executor DTO (mint key returned alongside).
fn from_row(r: AmmPoolFactRow) -> (String, AmmPoolFacts) {
    (
        r.mint_address,
        AmmPoolFacts {
            pool: r.pool,
            base_mint: r.base_mint,
            quote_mint: r.quote_mint,
            base_token_program: r.base_token_program,
            pool_base_token_account: r.pool_base_token_account,
            pool_quote_token_account: r.pool_quote_token_account,
            coin_creator: r.coin_creator,
            coin_creator_vault_ata: r.coin_creator_vault_ata,
            coin_creator_vault_authority: r.coin_creator_vault_authority,
            is_cashback_coin: r.is_cashback_coin,
            fee_share_marker: r.fee_share_marker,
            needs_pool_v2: r.needs_pool_v2,
        },
    )
}

/// Re-seed the trader's pool-facts cache for `mints` from the DB (boot path).
/// Zero RPC. Returns how many entries were seeded. Never fails boot — a DB error
/// just means the cache fills from live events / the cold path instead.
pub async fn seed_from_db(trader: &PumpFunTrader, pool: &PgPool, mints: &[String]) -> usize {
    if mints.is_empty() {
        return 0;
    }
    let repo = AmmPoolFactsRepo::new(pool.clone());
    let rows = match repo.find_for(mints).await {
        Ok(rows) => rows,
        Err(e) => {
            error!("amm_pool_facts seed query failed: {e}");
            return 0;
        }
    };
    let mut seeded = 0usize;
    for row in rows {
        let (mint, facts) = from_row(row);
        if trader.seed_amm_pool_facts(&mint, &facts) {
            seeded += 1;
        }
    }
    if seeded > 0 {
        info!(seeded, "seeded AMM pool facts from DB (zero RPC)");
    }
    seeded
}

/// Spawn the background loop that persists newly learned pool facts. Preloads the
/// set of already-stored mints so it only writes genuinely new pools (never a
/// re-write of an existing row, incl. the ones just seeded from DB on boot).
pub fn spawn_persist_loop(trader: Arc<PumpFunTrader>, pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let repo = AmmPoolFactsRepo::new(pool);
        let mut persisted: HashSet<String> = match repo.all_mints().await {
            Ok(m) => m.into_iter().collect(),
            Err(e) => {
                error!("amm_pool_facts preload failed (will still persist new pools): {e}");
                HashSet::new()
            }
        };
        let mut ticker = tokio::time::interval(PERSIST_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            for mint in trader.amm_pool_cached_mints() {
                if persisted.contains(&mint) {
                    continue;
                }
                let Some(facts) = trader.amm_pool_facts_snapshot(&mint) else {
                    continue;
                };
                match repo.upsert(&to_row(&mint, &facts)).await {
                    Ok(()) => {
                        persisted.insert(mint);
                    }
                    Err(e) => {
                        debug!(%mint, "amm_pool_facts upsert failed (retry next tick): {e}");
                    }
                }
            }
        }
    })
}
