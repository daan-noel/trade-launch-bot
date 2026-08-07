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
//!   [`Event::FirstSlotSettled`] the first time the creation slot has closed
//!   **while the token is still snipe-fresh**.
//! * `Migrated` ping → [`Event::Migrated`].
//!
//! **The restart rail (`started_at`).** The cursor lives in RAM, and the cold-start
//! cache seed backfills up to `SEED_TRADES_PER_MINT` historical rows per mint, so
//! after a restart a token's whole past reads as "new". Deciding on it is not a
//! cosmetic bug: the decision uses the old price while the fill uses the live one.
//! So every trade is split by chain time against [`Producer::started_at`] — older
//! is **primed** (folded into the metric track, no decision), newer is **decided**.
//! Nothing is lost: the 200 ms tick re-decides every token against the primed track.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use smallvec::SmallVec;

use hunter_engine::event::{Event, Mint};
use hunter_engine::grouping::LAMPORTS_PER_SOL_F64;
use hunter_engine::metrics::flow_split::wallet_hash;
use hunter_engine::token_identity_hash;
use hunter_engine::metrics::{Side, TradeLite};

use trading_core::config::constants::MAX_SNIPE_AGE_SECS;
use trading_core::models::ingest::{IngestKind, StrategyPing};
use trading_core::state::token_cache::{CachedTrade, TokenCache};

use super::convert::observed_axes;

/// A batch of engine events one ping produced (usually 0–2, rarely a burst of
/// backlogged trades).
pub type ProducedEvents = SmallVec<[Event; 4]>;
/// Historical trades one ping surfaced — folded into the track, never decided on.
pub type PrimedTrades = SmallVec<[(Mint, TradeLite); 4]>;

/// What one ping yields: trades to **observe** (history) and events to **decide**
/// on (live). The loop primes first, then folds — same order the trades happened.
#[derive(Default)]
pub struct Produced {
    /// Pre-boot trades: fold into the metric track via `hunter_engine::prime_trade`.
    pub prime: PrimedTrades,
    /// Live events: fold through `reduce`.
    pub events: ProducedEvents,
}

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
    /// When this engine loop started. **The history/live boundary**: a cached trade
    /// whose chain time predates it is replayed state, not a live signal — see
    /// [`Producer::split_trade`].
    started_at: DateTime<Utc>,
}

impl Producer {
    pub fn new(token_cache: Arc<TokenCache>, started_at: DateTime<Utc>) -> Self {
        Self {
            token_cache,
            trade_cursor: HashMap::new(),
            first_slot_emitted: HashSet::new(),
            migrated_emitted: HashSet::new(),
            started_at,
        }
    }

    /// Map one ping to engine events, reading the (already-updated) `TokenCache`.
    pub fn on_ping(&mut self, ping: &StrategyPing) -> Produced {
        match ping.kind {
            IngestKind::TokenCreated => Produced {
                events: self.on_token_created(&ping.mint),
                ..Default::default()
            },
            IngestKind::Trade => self.on_trade(&ping.mint),
            IngestKind::Migrated => Produced {
                events: self.on_migrated(&ping.mint),
                ..Default::default()
            },
            // Creator-activity pings don't drive an engine decision.
            IngestKind::CreatorActivity => Produced::default(),
        }
    }

    /// Prime a mint the engine tracks but this process has never produced for —
    /// the cold-start path for a token that may never trade again.
    ///
    /// An adopted position whose token has gone quiet gets **no** ping, so without
    /// this its track keeps a `NaN` price forever: `pnl`/`retrace`/`bounce` and the
    /// dead-token verdict all read `NaN`, which satisfies no condition, so the bag
    /// can never exit (only a `held` time-stop still works). Called from the tick,
    /// so it also closes the boot race against the async cache seed.
    /// Whether this process has already produced (or primed) `mint`'s trades — the
    /// cheap per-tick filter in front of [`Self::prime_tracked`].
    pub fn has_cursor(&self, mint: &str) -> bool {
        self.trade_cursor.contains_key(mint)
    }

    pub fn prime_tracked(&mut self, mint: &str) -> Produced {
        if self.has_cursor(mint) {
            return Produced::default();
        }
        // Not seeded yet ⇒ no cursor is written, so a later tick retries.
        self.drain_unseen(mint).unwrap_or_default()
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
        let creator_wallet_hash = (!token.creator_wallet.is_empty())
            .then(|| wallet_hash(&token.creator_wallet));
        out.push(Event::TokenCreated {
            mint: Mint::from(mint),
            fp: Box::new(tf),
            at,
            creator_wallet_hash,
            // The copycat key, straight off the create event's metadata — no
            // extra lookup, no RPC. `None` when either half is blank.
            identity: token_identity_hash(&token.name, &token.symbol),
        });
        out
    }

    fn on_trade(&mut self, mint: &str) -> Produced {
        let Some(mut out) = self.drain_unseen(mint) else {
            return Produced::default();
        };

        // The creation slot has closed (a later-slot trade landed): resolve any
        // deferred first-slot fingerprint. One-shot per mint, and only while the
        // token is still snipe-fresh — after a restart every seeded token's first
        // trade ping would otherwise settle a creation slot that closed hours ago.
        let (window_closed, buy_sol, sell_sol) = {
            let Some(entry) = self.token_cache.get(mint) else {
                return out;
            };
            let s = entry.value();
            let fresh = Utc::now().signed_duration_since(s.token.created_at).num_seconds()
                <= MAX_SNIPE_AGE_SECS;
            (
                fresh && !s.first_slot_window_open,
                s.first_slot_buy_sol,
                s.first_slot_sell_sol,
            )
        };
        if window_closed && !self.first_slot_emitted.contains(mint) {
            self.first_slot_emitted.insert(mint.to_string());
            out.events.push(Event::FirstSlotSettled {
                mint: Mint::from(mint),
                buy_lamports: sol_to_lamports_u64(buy_sol),
                sell_lamports: sol_to_lamports_u64(sell_sol),
                at: Utc::now(),
            });
        }
        out
    }

    /// Split every cached trade not yet seen for `mint` into history to prime and
    /// live events to decide on, and advance the cursor past all of them.
    /// `None` when the mint isn't cached (cursor untouched, so a later call retries).
    ///
    /// The cursor is absolute, so a dropped or coalesced ping is recovered here —
    /// we never key off `trades.last()` alone. The cursor lives only in RAM, so
    /// after a restart every seeded row reads as unseen: [`Self::split_trade`] is
    /// what stops that backlog from being decided on.
    fn drain_unseen(&mut self, mint: &str) -> Option<Produced> {
        let entry = self.token_cache.get(mint)?;
        let state = entry.value();
        let trades = Arc::clone(&state.trades);
        let trades_base = state.trades_base;
        drop(entry);

        let total = trades_base + trades.len() as u64;
        let cursor = self.trade_cursor.get(mint).copied().unwrap_or(trades_base);
        let start = cursor.saturating_sub(trades_base).min(trades.len() as u64) as usize;
        let mut out = Produced::default();
        for ct in &trades[start..] {
            self.split_trade(mint, ct, &mut out);
        }
        self.trade_cursor.insert(mint.to_string(), total);
        Some(out)
    }

    /// Route one cached trade: **history** (prime the track, no decision) when it
    /// happened before this loop started, **live** (an `Event::Trade` the engine
    /// decides on) otherwise.
    ///
    /// This one comparison is the whole restart rail. Everything the cache holds at
    /// boot — up to `SEED_TRADES_PER_MINT` rows per mint, reaching back
    /// `SEED_TRADES_MAX_AGE_HOURS` — predates `started_at`, so a warm start can
    /// never re-decide a token's past. Chain-time skew of a few seconds only ever
    /// misroutes a trade *into* history, where the next tick re-decides it against
    /// the same track — never the other way.
    fn split_trade(&self, mint: &str, ct: &CachedTrade, out: &mut Produced) {
        let trade = trade_lite(ct);
        if ct.block_time < self.started_at {
            out.prime.push((Mint::from(mint), trade));
        } else {
            out.events.push(Event::Trade { mint: Mint::from(mint), trade });
        }
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

/// One cached trade as the engine's `TradeLite`. The ONE conversion — the primed
/// and the decided path must describe a trade identically.
fn trade_lite(ct: &CachedTrade) -> TradeLite {
    TradeLite {
        side: if ct.is_buy { Side::Buy } else { Side::Sell },
        sol: ct.amount_sol,
        price: ct.price_per_token,
        // Deadness + liquidity read REAL reserves (SSOT parity with the live
        // `is_dead` signal); absent ⇒ NaN (no snapshot ⇒ alive).
        reserve_sol: ct.real_reserve_sol.unwrap_or(f64::NAN),
        at: ct.block_time,
        // Hashed once at cache ingest (`CachedTrade::from_trade`).
        ix_hash: ct.ix_hash,
        wallet_hash: ct.wallet_hash,
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    use trading_core::models::token::Token;
    use trading_core::models::trade::{Trade, TradeType};
    use trading_core::state::token_cache::TokenState;

    const MINT: &str = "MINT-producer-restart";

    fn token(created_ago_secs: i64) -> Token {
        Token::new(
            MINT.into(),
            "creator".into(),
            "Restart".into(),
            "RST".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            serde_json::Value::Array(vec![]),
            "create-sig".into(),
            None,
            Utc::now() - ChronoDuration::seconds(created_ago_secs),
        )
    }

    /// A trade `secs` before now (negative ⇒ after).
    fn trade(secs: i64, slot: u64) -> Trade {
        Trade::new(
            MINT.into(),
            "w".into(),
            TradeType::Buy,
            1.0,
            1_000_000,
            format!("sig-{slot}"),
            slot,
            Utc::now() - ChronoDuration::seconds(secs),
        )
    }

    fn cache_with(created_ago_secs: i64, trades: Vec<Trade>) -> Arc<TokenCache> {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token(created_ago_secs));
        for t in trades {
            state.add_trade(t);
        }
        cache.insert(MINT.into(), state);
        cache
    }

    fn ping() -> StrategyPing {
        StrategyPing { mint: MINT.into(), kind: IngestKind::Trade, received_at: None }
    }

    /// THE restart regression (2026-08-06, `247PRAda…`): after a restart the cache
    /// seed backfills the token's whole past and the in-RAM cursor is empty, so every
    /// seeded row reads as new. Those rows must be **primed**, never decided — a
    /// stop-loss that was true five minutes ago fills at today's price.
    #[test]
    fn seeded_history_is_primed_not_decided() {
        let cache = cache_with(600, vec![trade(500, 1), trade(400, 2), trade(300, 3)]);
        let mut p = Producer::new(cache, Utc::now());

        let out = p.on_ping(&ping());
        assert_eq!(out.prime.len(), 3, "every pre-boot trade primes");
        assert!(out.events.is_empty(), "no decision may be taken on history");
    }

    /// The live path is untouched: a trade that lands after the loop started is a
    /// signal, and still drives one `Event::Trade`.
    #[test]
    fn post_boot_trade_still_decides() {
        // Started a minute ago; the only trade is 10 s old ⇒ live.
        let cache = cache_with(120, vec![trade(10, 1)]);
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(60));

        let out = p.on_ping(&ping());
        assert!(out.prime.is_empty());
        assert_eq!(out.events.len(), 1);
        assert!(matches!(out.events[0], Event::Trade { .. }));
    }

    /// A backlog that straddles the boundary keeps chronological order across the
    /// two lanes: the old rows prime, only the fresh one decides.
    #[test]
    fn mixed_backlog_splits_at_started_at() {
        let cache = cache_with(300, vec![trade(200, 1), trade(100, 2), trade(5, 3)]);
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(30));

        let out = p.on_ping(&ping());
        assert_eq!(out.prime.len(), 2);
        assert_eq!(out.events.len(), 1);
    }

    /// The cold-start path for a token that may never print again: an adopted
    /// position gets no ping, so the tick primes it from the cache. Idempotent —
    /// the cursor is written, so the next tick is a no-op.
    #[test]
    fn prime_tracked_seeds_a_quiet_token_once() {
        let cache = cache_with(900, vec![trade(800, 1), trade(700, 2)]);
        let mut p = Producer::new(cache, Utc::now());

        let first = p.prime_tracked(MINT);
        assert_eq!(first.prime.len(), 2);
        assert!(first.events.is_empty());
        assert!(p.prime_tracked(MINT).prime.is_empty(), "second call is a no-op");
    }

    /// A mint the cache has not seeded yet must not latch a cursor, or the seed's
    /// rows would later be taken for live trades.
    #[test]
    fn prime_tracked_retries_until_the_cache_has_the_mint() {
        let cache = Arc::new(TokenCache::new());
        let mut p = Producer::new(cache.clone(), Utc::now());
        assert!(p.prime_tracked(MINT).prime.is_empty());

        let mut state = TokenState::new(token(900));
        state.add_trade(trade(800, 1));
        cache.insert(MINT.into(), state);
        assert_eq!(p.prime_tracked(MINT).prime.len(), 1, "retry picks up the seed");
    }

    /// A restart must not re-settle creation slots that closed hours ago: the
    /// first-slot event is gated on the same snipe-freshness rail as `TokenCreated`.
    #[test]
    fn stale_token_does_not_emit_first_slot_settled() {
        let cache = cache_with(MAX_SNIPE_AGE_SECS + 60, vec![trade(10, 1)]);
        {
            let mut e = cache.get_mut(MINT).unwrap();
            e.value_mut().first_slot_window_open = false;
        }
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(60));

        let out = p.on_ping(&ping());
        assert!(
            !out.events.iter().any(|e| matches!(e, Event::FirstSlotSettled { .. })),
            "a creation slot that closed long ago is not news"
        );
    }
}
