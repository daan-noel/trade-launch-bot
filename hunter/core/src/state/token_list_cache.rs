use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};

use crate::api::handlers::tokens::TokenSummary;

use super::token_cache::TokenCache;

/// Max age of the shared token-list snapshot before the next reader rebuilds it.
/// The Tokens table polls every 5s (plus an SSE-triggered refetch on new tokens),
/// so a 1s ceiling is invisible to the UI while collapsing every concurrent
/// request within the window onto a single rebuild.
const MAX_SNAPSHOT_STALENESS_MS: i64 = 1_000;

/// Immutable snapshot of the live cache (the tracked subset), shared by every
/// `/api/tokens` request: the tracked-only view and the `tracked` count. The full
/// list is never held here; it pages from Postgres (`handlers::tokens::list`).
pub struct TokenListSnapshot {
    /// Live cache rows, newest-first by `created_at`.
    live_rows: Vec<TokenSummary>,
    /// Wall-clock time the snapshot was materialised (drives the staleness check).
    built_at: DateTime<Utc>,
}

/// Default token-list order: newest-first by `created_at`, with a deterministic
/// `mint_address` DESC tail so a same-`created_at` pair never reorders across pages
/// or refetches. This mirrors the SQL engine's default `ORDER BY
/// t.created_at DESC, t.mint_address DESC` (`sql.rs::build_order`) exactly, so the
/// SQL and in-RAM engines page identically.
fn newest_first(a: &TokenSummary, b: &TokenSummary) -> std::cmp::Ordering {
    b.created_at
        .cmp(&a.created_at)
        .then_with(|| b.mint_address.cmp(&a.mint_address))
}

impl TokenListSnapshot {
    /// Clone the live cache into list rows and pre-sort newest-first. The cache is
    /// the small, post-eviction resident set, so a per-window rebuild stays cheap.
    fn build(cache: &TokenCache, now: DateTime<Utc>) -> Self {
        let mut live_rows: Vec<TokenSummary> =
            cache.iter().map(|e| TokenSummary::from(e.value())).collect();
        live_rows.sort_by(newest_first);
        Self { live_rows, built_at: now }
    }

    /// Count of live (cache-tracked) rows that satisfy `pred`. Reported beside the
    /// SQL `total` so the UI can show "tracked vs all".
    pub fn tracked_filtered_count(&self, mut pred: impl FnMut(&TokenSummary) -> bool) -> usize {
        self.live_rows.iter().filter(|t| pred(t)).count()
    }

    /// Rows currently resident in the cache, newest-first, keeping only those that
    /// satisfy `pred`. Used when `tracked_only=true` so the Tokens page can show
    /// just the actively-tracked subset.
    pub fn tracked_filtered(&self, mut pred: impl FnMut(&TokenSummary) -> bool) -> Vec<&TokenSummary> {
        self.live_rows.iter().filter(|t| pred(t)).collect()
    }
}

/// Shared, staleness-bounded snapshot of the live cache.
///
/// Each request reads the current `Arc<TokenListSnapshot>` for free; only when it
/// has aged past `MAX_SNAPSHOT_STALENESS_MS` does one reader rebuild it (others
/// block on the rebuild lock and then pick up the fresh result) — one live-cache
/// clone per staleness window regardless of how many clients poll.
pub struct TokenListCache {
    current: RwLock<Arc<TokenListSnapshot>>,
    /// Serialises rebuilds so a burst of stale readers triggers exactly one.
    rebuild_lock: Mutex<()>,
}

impl TokenListCache {
    /// Build an initial snapshot from the (possibly DB-seeded) cache so the first
    /// request is served without a rebuild stall.
    pub fn new(cache: &TokenCache) -> Self {
        let snap = Arc::new(TokenListSnapshot::build(cache, Utc::now()));
        Self {
            current: RwLock::new(snap),
            rebuild_lock: Mutex::new(()),
        }
    }

    /// Return a snapshot no older than `MAX_SNAPSHOT_STALENESS_MS`, rebuilding from
    /// `cache` if the current one has aged out. A rebuild clones the live cache, so
    /// call this from a blocking context (`web::block`).
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
        let rebuilt = Arc::new(TokenListSnapshot::build(cache, now));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `TokenSummary` — only mint + created_at matter here.
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
            creator_wallet: String::new(),
            creation_tx_signature: String::new(),
            name: String::new(),
            trade_count: 0,
            last_trade_at: None,
            lifetime_secs: None,
            last_synced_at: None,
        }
    }

    fn mints(rows: &[TokenSummary]) -> Vec<&str> {
        rows.iter().map(|t| t.mint_address.as_str()).collect()
    }

    #[test]
    fn newest_first_breaks_created_at_ties_by_mint_desc() {
        // A@10, C@10, B@10 share created_at → `mint_address DESC` (C > B > A),
        // matching the SQL default `created_at DESC, mint_address DESC`; D@30 leads.
        let mut rows = vec![ts("A", 10), ts("C", 10), ts("D", 30), ts("B", 10)];
        rows.sort_by(newest_first);
        assert_eq!(mints(&rows), vec!["D", "C", "B", "A"]);
    }
}
