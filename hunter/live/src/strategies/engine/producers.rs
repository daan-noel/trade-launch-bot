//! Producers — turn ingest `StrategyPing`s + the `TokenCache` into engine
//! [`Event`]s (plan 4.2). The loop calls [`Producer::on_ping`] per ping and folds
//! each returned event. This is the *only* place live wall-clock reads decide
//! which events exist; the engine stays pure.
//!
//! Event mapping (mirrors the old `runner.rs` dispatch, now emitting engine events):
//! * `TokenCreated` ping → [`Event::TokenCreated`] with the observed creation axes
//!   — **gated by [`token_is_fresh`]** so a gap-replayed old create never arms.
//! * `Trade` ping → one [`Event::Trade`] per *new* cached trade since a per-mint
//!   cursor (so a coalesced/dropped ping never silently loses flow), plus a
//!   [`Event::FirstSlotSettled`] the first time the creation slot has closed.
//! * `Migrated` ping → [`Event::Migrated`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use smallvec::SmallVec;

use hunter_engine::event::{Event, Mint};
use hunter_engine::grouping::LAMPORTS_PER_SOL_F64;
use hunter_engine::metrics::{Side, TradeLite};

use trading_core::config::constants::MAX_SNIPE_AGE_SECS;
use trading_core::models::ingest::{IngestKind, StrategyPing};
use trading_core::state::token_cache::TokenCache;

use super::convert::observed_axes;

/// A batch of engine events one ping produced (usually 0–2, rarely a burst of
/// backlogged trades).
pub type ProducedEvents = SmallVec<[Event; 4]>;

/// Per-mint producer state: the trade cursor + one-shot latches so duplicate pings
/// stay cheap and idempotent.
pub struct Producer {
    token_cache: Arc<TokenCache>,
    /// Absolute count of trades already turned into `Trade` events, per mint.
    trade_cursor: HashMap<String, u64>,
    /// Mints whose first-slot settlement has been emitted (one-shot).
    first_slot_emitted: HashSet<String>,
    /// Mints whose migration has been emitted (one-shot).
    migrated_emitted: HashSet<String>,
}

impl Producer {
    pub fn new(token_cache: Arc<TokenCache>) -> Self {
        Self {
            token_cache,
            trade_cursor: HashMap::new(),
            first_slot_emitted: HashSet::new(),
            migrated_emitted: HashSet::new(),
        }
    }

    /// Map one ping to engine events, reading the (already-updated) `TokenCache`.
    pub fn on_ping(&mut self, ping: &StrategyPing) -> ProducedEvents {
        match ping.kind {
            IngestKind::TokenCreated => self.on_token_created(&ping.mint),
            IngestKind::Trade => self.on_trade(&ping.mint),
            IngestKind::Migrated => self.on_migrated(&ping.mint),
            // Creator-activity pings don't drive an engine decision.
            IngestKind::CreatorActivity => ProducedEvents::new(),
        }
    }

    fn on_token_created(&mut self, mint: &str) -> ProducedEvents {
        let mut out = ProducedEvents::new();
        let Some(entry) = self.token_cache.get(mint) else {
            return out;
        };
        let token = entry.value().token.clone();
        drop(entry);

        // Live freshness rail (plan 4.2): a gap-replayed old create must never arm.
        // Deliberately not part of the fingerprint match set (analysis skips it).
        if Utc::now().signed_duration_since(token.created_at).num_seconds() > MAX_SNIPE_AGE_SECS {
            return out;
        }

        let at = token.created_at;
        let tf = observed_axes(&token, None, None);
        out.push(Event::TokenCreated { mint: Mint::from(mint), fp: Box::new(tf), at });
        out
    }

    fn on_trade(&mut self, mint: &str) -> ProducedEvents {
        let mut out = ProducedEvents::new();
        let Some(entry) = self.token_cache.get(mint) else {
            return out;
        };
        let state = entry.value();
        let trades = Arc::clone(&state.trades);
        let trades_base = state.trades_base;
        let window_closed = !state.first_slot_window_open;
        let first_slot_buy_sol = state.first_slot_buy_sol;
        let first_slot_sell_sol = state.first_slot_sell_sol;
        drop(entry);

        // Emit every trade not yet seen (absolute index ≥ cursor). A dropped or
        // coalesced ping is recovered here — we never key off `trades.last()` alone.
        let total = trades_base + trades.len() as u64;
        let cursor = self.trade_cursor.get(mint).copied().unwrap_or(trades_base);
        let start = cursor.saturating_sub(trades_base).min(trades.len() as u64) as usize;
        for ct in &trades[start..] {
            out.push(Event::Trade {
                mint: Mint::from(mint),
                trade: TradeLite {
                    side: if ct.is_buy { Side::Buy } else { Side::Sell },
                    sol: ct.amount_sol,
                    price: ct.price_per_token,
                    // Deadness + liquidity read REAL reserves (SSOT parity with the
                    // live `is_dead` signal); absent ⇒ NaN (no snapshot ⇒ alive).
                    reserve_sol: ct.real_reserve_sol.unwrap_or(f64::NAN),
                    at: ct.block_time,
                },
            });
        }
        self.trade_cursor.insert(mint.to_string(), total);

        // The creation slot has closed (a later-slot trade landed): resolve any
        // deferred first-slot fingerprint. One-shot per mint.
        if window_closed && !self.first_slot_emitted.contains(mint) {
            self.first_slot_emitted.insert(mint.to_string());
            out.push(Event::FirstSlotSettled {
                mint: Mint::from(mint),
                buy_lamports: sol_to_lamports_u64(first_slot_buy_sol),
                sell_lamports: sol_to_lamports_u64(first_slot_sell_sol),
                at: Utc::now(),
            });
        }
        out
    }

    fn on_migrated(&mut self, mint: &str) -> ProducedEvents {
        let mut out = ProducedEvents::new();
        if self.migrated_emitted.contains(mint) {
            return out;
        }
        self.migrated_emitted.insert(mint.to_string());
        out.push(Event::Migrated { mint: Mint::from(mint), at: Utc::now() });
        out
    }

    /// Drop per-mint producer state once the engine no longer tracks the mint
    /// (bounds the maps to the tracked set). Called by the loop on token prune.
    pub fn forget(&mut self, mint: &str) {
        self.trade_cursor.remove(mint);
        self.first_slot_emitted.remove(mint);
        self.migrated_emitted.remove(mint);
    }

    /// Bound the per-mint maps to the still-tracked set (called periodically by the
    /// loop with the engine's live mints), so a token that traded once and was
    /// pruned doesn't leak a cursor for the process lifetime.
    pub fn retain<F: Fn(&str) -> bool>(&mut self, keep: F) {
        self.trade_cursor.retain(|m, _| keep(m));
        self.first_slot_emitted.retain(|m| keep(m));
        self.migrated_emitted.retain(|m| keep(m));
    }
}

/// Human-SOL → lamports (`u64`, saturating at 0) for the first-slot axes the
/// fingerprint matcher buckets. The cache holds these as human SOL.
fn sol_to_lamports_u64(sol: f64) -> u64 {
    if sol.is_finite() && sol > 0.0 {
        (sol * LAMPORTS_PER_SOL_F64).round() as u64
    } else {
        0
    }
}
