use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tracing::warn;

use crate::api::handlers::tokens::TokenSummary;
use crate::config::constants::TOKEN_LIST_DB_REFRESH_SECS;
use crate::storage::repositories::token_repo::TokenRepo;

use super::token_cache::TokenCache;

/// Max age of the shared token-list snapshot before the next reader rebuilds it.
/// The Tokens table polls every 5s (plus an SSE-triggered refetch on new tokens),
/// so a 1s ceiling is invisible to the UI while collapsing every concurrent
/// request within the window onto a single rebuild.
const MAX_SNAPSHOT_STALENESS_MS: i64 = 1_000;

/// Immutable token-list snapshot shared by every `GET /api/tokens` request. Two
/// sources, both pre-sorted newest-first by `created_at`:
///   - `live_rows`: the live cache (fresh stats), rebuilt each staleness window.
///   - `db_rows`: the DB base (`tokens LEFT JOIN tokens_info`, windowed+capped like
///     the cold-start seed), refreshed on a slow background timer and shared by
///     `Arc` so a per-window rebuild never deep-clones it.
///
/// The list is the live cache **overlaying** the DB base: a mint resident in the
/// cache is served from `live_rows` (fresher), and its `db_rows` copy is skipped.
/// Mints only in `db_rows` — created-but-never-cached, or evicted after going idle
/// (`TOKEN_CACHE_EVICT_IDLE_SECONDS`) — still appear, so the page reflects the whole
/// seeded universe rather than just the currently-tracked subset.
pub struct TokenListSnapshot {
    /// Live cache rows, newest-first by `created_at`.
    live_rows: Vec<TokenSummary>,
    /// Mints present in `live_rows`; used to shadow the DB base so a tracked mint
    /// is never emitted twice (the live copy wins).
    live_mints: HashSet<String>,
    /// Shared DB base, newest-first by `created_at`.
    db_rows: Arc<Vec<TokenSummary>>,
    /// Wall-clock time the snapshot was materialised (drives the staleness check).
    built_at: DateTime<Utc>,
}

impl TokenListSnapshot {
    /// Clone the live cache into list rows and pre-sort newest-first, pairing it
    /// with the shared (Arc) DB base. Only the live half is cloned here — the DB
    /// half is a refcount bump — so a per-window rebuild stays proportional to the
    /// (small, post-eviction) resident set, not the whole seeded universe.
    fn build(cache: &TokenCache, db_rows: Arc<Vec<TokenSummary>>, now: DateTime<Utc>) -> Self {
        let mut live_rows: Vec<TokenSummary> =
            cache.iter().map(|e| TokenSummary::from(e.value())).collect();
        live_rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let live_mints = live_rows.iter().map(|t| t.mint_address.clone()).collect();
        Self {
            live_rows,
            live_mints,
            db_rows,
            built_at: now,
        }
    }

    /// Count of live (cache-tracked) rows that satisfy `pred`. Reported beside the
    /// merged `total` so the UI can show "tracked vs all". Scans only `live_rows`
    /// (the small resident set), never the DB base.
    pub fn tracked_filtered_count(&self, mut pred: impl FnMut(&TokenSummary) -> bool) -> usize {
        self.live_rows.iter().filter(|t| pred(t)).count()
    }

    /// Live-only view — only rows currently resident in the cache — newest-first,
    /// keeping only those that satisfy `pred`. Used when `tracked_only=true` so the
    /// Tokens page can show just the actively-tracked subset without the DB base.
    pub fn tracked_filtered<'a>(
        &'a self,
        mut pred: impl FnMut(&TokenSummary) -> bool,
    ) -> Vec<&'a TokenSummary> {
        self.live_rows.iter().filter(|t| pred(t)).collect()
    }

    /// Merged view — live cache overlaying the DB base — newest-first by
    /// `created_at`, keeping only rows that satisfy `pred`. A two-pointer merge of
    /// the two desc-sorted sources: it returns borrows (no clone), and DB rows
    /// whose mint is in the live set are dropped (the live copy already covers that
    /// mint). The result is globally newest-first, so the default (unsorted) table
    /// view needs no further sort.
    pub fn merged_filtered<'a>(
        &'a self,
        mut pred: impl FnMut(&TokenSummary) -> bool,
    ) -> Vec<&'a TokenSummary> {
        let live = &self.live_rows;
        let db = &self.db_rows;
        let mut out = Vec::with_capacity(live.len() + db.len());
        let (mut i, mut j) = (0usize, 0usize);
        while i < live.len() && j < db.len() {
            // Skip DB rows shadowed by a live (fresher) copy of the same mint.
            if self.live_mints.contains(&db[j].mint_address) {
                j += 1;
                continue;
            }
            if live[i].created_at >= db[j].created_at {
                if pred(&live[i]) {
                    out.push(&live[i]);
                }
                i += 1;
            } else {
                if pred(&db[j]) {
                    out.push(&db[j]);
                }
                j += 1;
            }
        }
        while i < live.len() {
            if pred(&live[i]) {
                out.push(&live[i]);
            }
            i += 1;
        }
        while j < db.len() {
            if !self.live_mints.contains(&db[j].mint_address) && pred(&db[j]) {
                out.push(&db[j]);
            }
            j += 1;
        }
        out
    }
}

/// Shared, staleness-bounded snapshot of the token list.
///
/// Replaces the old per-request, per-client full-cache clone: each request reads
/// the current `Arc<TokenListSnapshot>` for free; only when it has aged past
/// `MAX_SNAPSHOT_STALENESS_MS` does one reader rebuild it (others block on the
/// rebuild lock and then pick up the fresh result). The DB base is held separately
/// and shared into each snapshot by `Arc`, so a rebuild only re-clones the small
/// live-cache half — total cost is one live-cache clone per staleness window
/// regardless of how many clients poll or how large the seeded universe is.
pub struct TokenListCache {
    current: RwLock<Arc<TokenListSnapshot>>,
    /// DB-backed base of the list (whole seeded universe), refreshed by
    /// `run_token_list_db_refresh`. Shared into every snapshot by `Arc`.
    db_base: RwLock<Arc<Vec<TokenSummary>>>,
    /// Serialises rebuilds so a burst of stale readers triggers exactly one.
    rebuild_lock: Mutex<()>,
}

impl TokenListCache {
    /// Build an initial snapshot from the (possibly DB-seeded) cache so the first
    /// request is served without a rebuild stall. The DB base starts empty and is
    /// populated by the background refresher's immediate first tick.
    pub fn new(cache: &TokenCache) -> Self {
        let db_base: Arc<Vec<TokenSummary>> = Arc::new(Vec::new());
        let snap = Arc::new(TokenListSnapshot::build(cache, db_base.clone(), Utc::now()));
        Self {
            current: RwLock::new(snap),
            db_base: RwLock::new(db_base),
            rebuild_lock: Mutex::new(()),
        }
    }

    /// Replace the DB base. Called by `run_token_list_db_refresh`; the next
    /// snapshot rebuild (within `MAX_SNAPSHOT_STALENESS_MS`) picks it up.
    pub fn set_db_base(&self, rows: Vec<TokenSummary>) {
        *self.db_base.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(rows);
    }

    /// Return a snapshot no older than `MAX_SNAPSHOT_STALENESS_MS`, rebuilding from
    /// `cache` (+ the current DB base) if the current one has aged out. A rebuild
    /// clones the live cache, so call this from a blocking context (`web::block`).
    pub fn get(&self, cache: &TokenCache, now: DateTime<Utc>) -> Arc<TokenListSnapshot> {
        if let Some(fresh) = self.fresh(now) {
            return fresh;
        }
        // Stale: one reader rebuilds under the lock; concurrent readers block here
        // and then pick up the rebuilt snapshot via the re-check below (so a burst
        // of polls collapses to a single rebuild).
        let _guard = self.rebuild_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(fresh) = self.fresh(now) {
            return fresh;
        }
        let db_rows = self
            .db_base
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let rebuilt = Arc::new(TokenListSnapshot::build(cache, db_rows, now));
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = rebuilt.clone();
        rebuilt
    }

    /// The current snapshot if still within the staleness window, else `None`.
    fn fresh(&self, now: DateTime<Utc>) -> Option<Arc<TokenListSnapshot>> {
        let cur = self
            .current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let age = now.signed_duration_since(cur.built_at).num_milliseconds();
        if age < MAX_SNAPSHOT_STALENESS_MS {
            Some(cur)
        } else {
            None
        }
    }
}

/// Periodically refresh the token-list DB base so `GET /api/tokens` reflects the
/// whole in-RAM snapshot universe (`tokens LEFT JOIN tokens_info`, windowed by
/// `window_days` + capped at `limit`) — including mints already evicted from the
/// live cache — overlaid with live stats for tracked mints. One bounded,
/// index-servable query per `TOKEN_LIST_DB_REFRESH_SECS`; the base matters only for
/// idle/evicted mints (static stats), so a coarse cadence is plenty. The first tick
/// fires immediately so the base is populated right after startup; until then the
/// list serves the live cache alone.
///
/// **Lab-only in the split.** `lab` runs this with `LAB_TOKEN_LIST_LIMIT` /
/// `LAB_TOKEN_LIST_WINDOW_DAYS` (workstation RAM, wants the whole universe resident
/// for fast analysis). `live` no longer spawns it at all — the live box pages the
/// full list straight from Postgres and keeps this snapshot tracking-only (the DB
/// base stays empty), respecting the 4 GB EC2 guardrail.
pub async fn run_token_list_db_refresh(
    token_repo: TokenRepo,
    token_list: Arc<TokenListCache>,
    limit: i64,
    window_days: i64,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(TOKEN_LIST_DB_REFRESH_SECS));
    loop {
        tick.tick().await;
        let since = Utc::now() - ChronoDuration::days(window_days);
        match token_repo.find_list_rows(limit, since).await {
            Ok(rows) => {
                let summaries: Vec<TokenSummary> =
                    rows.into_iter().map(TokenSummary::from).collect();
                token_list.set_db_base(summaries);
            }
            Err(e) => warn!("token-list DB base refresh failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `TokenSummary` for merge tests — only mint + created_at matter here.
    fn ts(mint: &str, created_secs: i64) -> TokenSummary {
        let created_at = DateTime::<Utc>::from_timestamp(created_secs, 0).unwrap();
        TokenSummary {
            mint_address: mint.to_string(),
            symbol: String::new(),
            current_price: None,
            ath_price: None,
            ath_timestamp: None,
            volume_sol_total: 0.0,
            first_slot_buy_sol: None,
            first_slot_sell_sol: None,
            market_cap: None,
            initial_buy_sol: None,
            initial_supply_token: None,
            token_amount: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            min_tokens_out: None,
            cu_limit: None,
            cu_price: None,
            is_mayhem_mode: false,
            is_cashback_enabled: false,
            ix_labels_count: 0,
            instruction_labels: serde_json::Value::Array(vec![]),
            is_migrated: false,
            is_dead: false,
            age_seconds: 0,
            created_at,
            creator_address: String::new(),
            create_tx_address: String::new(),
            name: String::new(),
            trade_count: 0,
            last_trade_at: None,
            lifetime_secs: None,
            last_synced_at: None,
        }
    }

    fn snapshot(live: Vec<TokenSummary>, db: Vec<TokenSummary>) -> TokenListSnapshot {
        let live_mints = live.iter().map(|t| t.mint_address.clone()).collect();
        TokenListSnapshot {
            live_rows: live,
            live_mints,
            db_rows: Arc::new(db),
            built_at: Utc::now(),
        }
    }

    fn mints<'a>(rows: &[&'a TokenSummary]) -> Vec<&'a str> {
        rows.iter().map(|t| t.mint_address.as_str()).collect()
    }

    #[test]
    fn merges_desc_and_dedups_live_over_db() {
        // live newest-first: B@30, D@10.  db newest-first: A@40, B@25 (shadowed), C@5.
        let live = vec![ts("B", 30), ts("D", 10)];
        let db = vec![ts("A", 40), ts("B", 25), ts("C", 5)];
        let snap = snapshot(live, db);

        let out = snap.merged_filtered(|_| true);
        // Globally newest-first; B served once from the live copy (created_at 30,
        // not the DB's stale 25), placing it after A and before D.
        assert_eq!(mints(&out), vec!["A", "B", "D", "C"]);
    }

    #[test]
    fn predicate_filters_both_sources() {
        let live = vec![ts("B", 30)];
        let db = vec![ts("A", 40), ts("C", 5)];
        let snap = snapshot(live, db);
        let out = snap.merged_filtered(|t| t.mint_address != "A");
        assert_eq!(mints(&out), vec!["B", "C"]);
    }

    #[test]
    fn empty_db_base_is_live_only() {
        // Before the first DB refresh the base is empty → prior behaviour: the
        // snapshot serves the live cache alone.
        let snap = snapshot(vec![ts("B", 30), ts("A", 40)], vec![]);
        let out = snap.merged_filtered(|_| true);
        assert_eq!(out.len(), 2);
    }
}
