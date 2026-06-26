//! Cross-run trade-history cache for backtests.
//!
//! Re-running a simulation while tuning a rule re-fetches the *same* immutable
//! trade history for every candidate token — the dominant cost of a backtest.
//! This caches each mint's full DB history (`Arc<Vec<Trade>>`) keyed by mint, so
//! a second run over an unchanged candidate set issues no `trades` query at all.
//!
//! Freshness is exact and free: every entry remembers the token's
//! [`TokenState::trade_count`](crate::state::token_cache::TokenState) (total
//! trades ever seen, in-memory, bumped only when a new trade lands). A `get`
//! supplies the *current* count; if it differs, new trades arrived since the
//! fetch and the entry is dropped — no TTL, no validation round-trip. A token
//! with no new activity stays cached indefinitely.
//!
//! Memory is bounded by total cached trades (`MAX_CACHED_TRADES`) with FIFO
//! eviction: the working set of a tuning session is stable across runs, so
//! insertion-order eviction is as good as LRU here while staying O(1) per op (no
//! per-`get` reorder). Stale entries removed by `get` leave a tombstone in the
//! eviction queue that is skipped when it surfaces — eviction never scans.
//!
//! Only the backtest touches this; it is never on the ingest/strategy hot path,
//! so a single `Mutex` (uncontended — a few concurrent chunk futures) is fine.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::models::trade::Trade;

/// Upper bound on the total number of `Trade`s held across all cached mints.
/// ~500k trades ≈ a couple hundred MB; a backtest accelerator, sized to leave
/// the box's RAM to the live ingest caches. Tune to the host if needed.
const MAX_CACHED_TRADES: usize = 500_000;

struct Entry {
    trades: Arc<Vec<Trade>>,
    /// `TokenState::trade_count` at fetch time — the freshness key.
    trade_count: u64,
}

struct Inner {
    map: HashMap<String, Entry>,
    /// Insertion order for FIFO eviction. May hold tombstones (mints no longer in
    /// `map`); they are skipped when popped.
    order: VecDeque<String>,
    total_trades: usize,
}

pub struct BacktestTradeCache {
    inner: Mutex<Inner>,
}

impl Default for BacktestTradeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl BacktestTradeCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                order: VecDeque::new(),
                total_trades: 0,
            }),
        }
    }

    /// Return the cached history for `mint` iff it is still fresh, i.e. the token's
    /// `trade_count` is unchanged since the entry was stored. A stale entry is
    /// evicted (and `None` returned) so the caller refetches.
    pub fn get(&self, mint: &str, current_trade_count: u64) -> Option<Arc<Vec<Trade>>> {
        let mut g = self.inner.lock().expect("backtest cache poisoned");
        match g.map.get(mint) {
            Some(e) if e.trade_count == current_trade_count => Some(e.trades.clone()),
            Some(_) => {
                // New trades landed since the fetch — drop the stale history. The
                // order-queue tombstone is reclaimed lazily on eviction.
                let removed = g.map.remove(mint).expect("just matched");
                g.total_trades -= removed.trades.len();
                None
            }
            None => None,
        }
    }

    /// Store `mint`'s freshly fetched history, stamped with the token's
    /// `trade_count` at fetch time. Evicts oldest entries until back under the
    /// trade-count cap.
    pub fn insert(&self, mint: String, trades: Arc<Vec<Trade>>, trade_count: u64) {
        let len = trades.len();
        let mut g = self.inner.lock().expect("backtest cache poisoned");

        if let Some(old) = g.map.insert(mint.clone(), Entry { trades, trade_count }) {
            g.total_trades -= old.trades.len();
        }
        g.total_trades += len;
        g.order.push_back(mint);

        while g.total_trades > MAX_CACHED_TRADES {
            let Some(old_mint) = g.order.pop_front() else { break };
            if let Some(e) = g.map.remove(&old_mint) {
                g.total_trades -= e.trades.len();
            }
            // else: tombstone for an already-removed (stale) entry — skip.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::models::trade::{Trade, TradeType};

    fn history(mint: &str, n: usize) -> Arc<Vec<Trade>> {
        Arc::new(
            (0..n)
                .map(|i| {
                    Trade::new(
                        mint.to_string(),
                        "W".to_string(),
                        TradeType::Buy,
                        0.1,
                        100.0,
                        format!("{mint}-{i}"),
                        i as u64,
                        Utc::now(),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn hit_only_when_trade_count_unchanged() {
        let cache = BacktestTradeCache::new();
        cache.insert("M".to_string(), history("M", 3), 3);

        // Same count → fresh hit.
        assert!(cache.get("M", 3).is_some(), "unchanged count is a hit");

        // A new trade landed (count advanced) → stale, evicted, miss.
        assert!(cache.get("M", 4).is_none(), "advanced count invalidates");
        // The stale entry was dropped, so even the original count now misses.
        assert!(cache.get("M", 3).is_none(), "stale entry removed on access");
    }

    #[test]
    fn fifo_eviction_bounds_total_trades() {
        let cache = BacktestTradeCache::new();
        // Two oversized inserts: the first must be evicted to fit the second.
        let big = MAX_CACHED_TRADES - 10;
        cache.insert("A".to_string(), history("A", big), 1);
        cache.insert("B".to_string(), history("B", 100), 1);

        assert!(cache.get("A", 1).is_none(), "oldest evicted to honor the cap");
        assert!(cache.get("B", 1).is_some(), "newest retained");
    }
}
