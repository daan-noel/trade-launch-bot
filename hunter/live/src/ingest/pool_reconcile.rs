//! The one owner of the tracked AMM pool set.
//!
//! # Why this exists
//!
//! The pool index had no owner. Three call sites added to it for three unrelated
//! reasons — the migration decode auto-registers
//! (`ingest_pumpfun::decode::protobuf`), a held position needs its pool for
//! sell-confirm ([`HeldPoolGate`]), a UI "Fetch New" on a migrated token wants a
//! live view (`services::token_sync`) — and three unrelated one-shot events
//! removed from it. Nothing could answer "should this pool be subscribed *right
//! now*?", because the set never recorded why anything was in it.
//!
//! That one gap produced both failure modes at once:
//!
//! - **Cost.** A pool that nothing removes stays on the metered gRPC filter for
//!   the life of the process, and (since a subscription carrying transactions
//!   also asks for block metas) drags ~2.5 frames/s back with it. `token_sync`
//!   even promised the cleanup — *"the next reconnect re-prunes it if it's since
//!   gone quiet"* — but no such prune existed: `pool_is_live` had exactly one
//!   caller, inside the branch that runs only when `track_post_migration` is ON.
//! - **Stability.** With no owner, a prune written against the in-memory held set
//!   could drop the pool of a real bag, because that set drifts: `release`
//!   returns early for a mint it never saw, and a settle that happens while the
//!   process is down is never released at all.
//!
//! # The rule
//!
//! One task, one decision, every [`RECONCILE_INTERVAL`]. The desired set is
//! computed from scratch and the index is moved to match it:
//!
//! ```text
//! keep a pool when
//!     the mint has an unsettled REAL position          (authoritative: the DB)
//!  OR track_post_migration is on AND the pool is live  (the operator asked to record)
//!  OR it entered the index less than GRACE ago         (a just-added pool gets its window)
//! ```
//!
//! **The held arm is read from `strategy_positions` on every pass, not from the
//! accumulated event history.** That is what makes the prune safe: the sweep
//! cannot blind an exit, because the thing it checks against is re-derived from
//! the same rows the exit path itself is driven by. A held set that drifted down
//! would be repaired by the next pass, in the direction of *more* subscription.
//!
//! Both directions are applied — this task also *adds* a held pool that nothing
//! subscribed (a bag whose migration event was missed), which is a pure exit-path
//! win independent of the cost one.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use dashmap::DashMap;
use ingest_pumpfun::IngestHandle;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use trading_core::{
    config::constants::POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS,
    state::token_cache::TokenCache,
    storage::repositories::{settings_repo::AppSettings, strategy_repo::StrategyRepo},
};

use super::held_pools::HeldPoolGate;

/// How often the desired set is recomputed.
///
/// One indexed `SELECT DISTINCT` over `strategy_positions`, off every hot path.
/// 30 s is far inside the window where losing an AMM pool subscription could
/// matter to an exit, and slow enough that a leaking pool costs at most one
/// interval of traffic beyond its grace period.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

/// How long a newly-seen pool is kept regardless of the rules above.
///
/// A pool arrives before anything knows what it is for: the decode auto-registers
/// on the migration frame, and a "Fetch New" registers so the operator gets a
/// live view immediately. Pruning either on the very next pass would make both
/// features useless. Five minutes is long enough to be worth having and short
/// enough that an unclaimed pool is not a subscription.
const GRACE: Duration = Duration::from_secs(300);

/// First time this task saw a given pool in the index — the clock behind [`GRACE`].
type FirstSeen = Arc<DashMap<String, Instant>>;

/// Spawn the reconciler. Runs until the process ends.
#[allow(clippy::too_many_arguments)]
pub fn spawn_pool_reconciler(
    ingest: Arc<IngestHandle>,
    held: HeldPoolGate,
    token_cache: Arc<TokenCache>,
    repo: StrategyRepo,
    settings_rx: watch::Receiver<AppSettings>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let first_seen: FirstSeen = Arc::new(DashMap::new());
        let mut tick = tokio::time::interval(RECONCILE_INTERVAL);
        tick.tick().await; // consume the immediate first tick
        loop {
            tick.tick().await;
            if let Err(e) = reconcile_once(&ingest, &held, &token_cache, &repo, &settings_rx, &first_seen).await
            {
                // Never fatal: a failed pass changes nothing, and the next one
                // re-derives from scratch. Failing CLOSED (touching nothing) is
                // the safe direction — the alternative is pruning against a held
                // set we could not read.
                warn!("pool reconcile: pass failed, index left as-is — {e}");
            }
        }
    })
}

async fn reconcile_once(
    ingest: &Arc<IngestHandle>,
    held: &HeldPoolGate,
    token_cache: &Arc<TokenCache>,
    repo: &StrategyRepo,
    settings_rx: &watch::Receiver<AppSettings>,
    first_seen: &FirstSeen,
) -> anyhow::Result<()> {
    // 1. Two authoritative sets, and they are NOT the same question.
    //
    //    `real` — could the wallet be holding a bag? That is what `HeldPoolGate`
    //    means, what the boot seed uses, and what the consumer's RPC-warm and
    //    sell-confirm decisions read. Propagated into the gate so every
    //    `contains` caller downstream answers off these rows rather than off an
    //    event history that can have drifted.
    //
    //    `any` — does anything still need this mint's price feed? A paper exit
    //    resolves from the same `trades` rows a real one does, so pruning a
    //    migrated mint's pool blinds a paper run exactly like a real one. This is
    //    the retention set, and it is a superset of `real`.
    let real = repo.distinct_unsettled_real_mints().await?;
    let unsettled = repo.distinct_unsettled_mints().await?;
    let newly_held = held.replace(&real);
    if !newly_held.is_empty() {
        // A bag whose pool nothing subscribed — a missed migration event, or a
        // position adopted from a restart. Subscribing is the exit-path fix and
        // it happens before any pruning below.
        warn!(
            n = newly_held.len(),
            "pool reconcile: unsettled position(s) whose pool was not tracked — subscribing"
        );
        ingest.track_pools(&newly_held);
    }

    let track_post_migration = settings_rx.borrow().track_post_migration;
    let holding: HashSet<&str> = unsettled.iter().map(String::as_str).collect();
    let now = Utc::now();

    // 2. Decide, per pool currently in the index, whether it still earns its place.
    let index = ingest.pool_index();
    let mut drop_mints: Vec<String> = Vec::new();
    for entry in index.iter() {
        let (pool, mint) = (entry.key(), entry.value());
        let seen = *first_seen
            .entry(pool.clone())
            .or_insert_with(Instant::now)
            .value();

        if holding.contains(mint.as_str()) {
            continue; // a position depends on this feed — never a candidate
        }
        if seen.elapsed() < GRACE {
            continue; // too new to judge
        }
        if track_post_migration && pool_is_live(token_cache, mint, now) {
            continue; // the operator asked to record live post-migration traffic
        }
        drop_mints.push(mint.clone());
    }

    // 3. Apply. `untrack_pools` fires `pools_changed`, so one resubscribe covers
    //    the batch.
    if !drop_mints.is_empty() {
        ingest.untrack_pools(&drop_mints);
        info!(
            dropped = drop_mints.len(),
            remaining = index.len(),
            unsettled = unsettled.len(),
            held_real = real.len(),
            track_post_migration,
            "pool reconcile: untracked pool(s) no position or setting asks for"
        );
    }

    // 4. Forget the clock for pools that are gone, so the map cannot grow without
    //    bound across a long uptime. Snapshot the keys FIRST: `retain` holds a
    //    `first_seen` shard while its predicate runs, and reading `index` from
    //    inside it would take the two maps in the opposite order from the loop
    //    above. dashmap 4.0's shard lock is an unbounded spinlock, so lock-order
    //    inversions here are livelocks, not waits.
    let live: HashSet<String> = index.iter().map(|e| e.key().clone()).collect();
    first_seen.retain(|pool, _| live.contains(pool));
    Ok(())
}

/// Whether this mint has traded inside the activity window.
///
/// Absent from the cache, or never traded, reads as NOT live — the same verdict
/// `consumer::pool_is_live` gives, and the conservative one: an unknown pool is
/// one nothing has asked for.
fn pool_is_live(token_cache: &Arc<TokenCache>, mint: &str, now: chrono::DateTime<Utc>) -> bool {
    token_cache
        .get(mint)
        .and_then(|s| s.last_trade_at)
        .is_some_and(|last| (now - last).num_seconds() <= POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS)
}
