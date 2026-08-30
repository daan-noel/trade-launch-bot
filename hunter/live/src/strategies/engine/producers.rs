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
use hunter_engine::metrics::flow_ix::wallet_hash;
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
    ///
    /// Keyed by [`Mint`], not `String`, so it doubles as the mint interner: every
    /// event this producer emits clones the key (an `Arc` refcount bump) instead of
    /// allocating and copying the 44-byte address again. A trade ping can carry a
    /// burst of trades, and each one used to pay that allocation.
    trade_cursor: HashMap<Mint, u64>,
    /// Mints whose first-slot settlement has been emitted (one-shot).
    first_slot_emitted: HashSet<String>,
    /// Highest slot seen on any drained trade — the proof a creation slot has
    /// closed. The feed delivers per block, so observing slot `S+1` anywhere means
    /// every slot-`S` trade has already been delivered. Waiting instead for a
    /// later-slot trade **on the token itself** costs the slots the token is quiet
    /// for, which on a bundled launch is exactly where the edge is.
    slot_watermark: u64,
    /// Mints awaiting first-slot settlement, keyed to their creation slot. Bounded
    /// by the settle sweep: an entry leaves the moment the watermark passes it, or
    /// when the token ages past the snipe rail.
    pending_first_slot: HashMap<String, u64>,
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
            slot_watermark: 0,
            pending_first_slot: HashMap::new(),
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
        // Queue the deferred first-slot resolve. The watermark sweep settles it as
        // soon as any token trades in a later slot, so a quiet launch no longer waits
        // for its own next trade.
        if let Some(creation_slot) = token.creation_slot {
            if !self.first_slot_emitted.contains(mint) {
                self.pending_first_slot.insert(mint.to_string(), creation_slot);
            }
        }
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

        // The creation slot has closed for this token (a later-slot trade landed on
        // it): resolve any deferred first-slot fingerprint. The tick's watermark
        // sweep usually gets there first — this stays as the path for a mint that
        // never went through `on_token_created` in this process (an adopted or
        // log-re-armed token, whose creation slot the producer never queued).
        let window_closed = {
            let Some(entry) = self.token_cache.get(mint) else {
                return out;
            };
            !entry.value().first_slot_window_open
        };
        if window_closed {
            self.pending_first_slot.remove(mint);
            if let Some(ev) = self.settle_first_slot(mint) {
                out.events.push(ev);
            }
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
        // The cursor map is also the interner: take the key back when it exists, so
        // this mint is turned into a `Mint` once for the life of the process rather
        // than once per event.
        let (key, cursor) = match self.trade_cursor.get_key_value(mint) {
            Some((k, &c)) => (k.clone(), c),
            None => (Mint::from(mint), trades_base),
        };
        let start = cursor.saturating_sub(trades_base).min(trades.len() as u64) as usize;
        let mut out = Produced::default();
        for ct in &trades[start..] {
            self.slot_watermark = self.slot_watermark.max(ct.slot);
            self.split_trade(&key, ct, &mut out);
        }
        self.trade_cursor.insert(key, total);
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
    fn split_trade(&self, mint: &Mint, ct: &CachedTrade, out: &mut Produced) {
        let trade = trade_lite(ct);
        if ct.block_time < self.started_at {
            out.prime.push((mint.clone(), trade));
        } else {
            out.events.push(Event::Trade { mint: mint.clone(), trade });
        }
    }

    /// Emit `FirstSlotSettled` for `mint` once, reading the creation-slot sums the
    /// cache has accumulated. `None` when already emitted, uncached, or past the
    /// snipe-freshness rail (a restart must not re-settle slots that closed hours
    /// ago). Read-only on the cache: a late same-slot trade may still grow the sums,
    /// but nothing reads them after the event — the same tolerance
    /// `first_slot_window_open` already documents.
    fn settle_first_slot(&mut self, mint: &str) -> Option<Event> {
        if self.first_slot_emitted.contains(mint) {
            return None;
        }
        let (fresh, buy_sol, sell_sol) = {
            let entry = self.token_cache.get(mint)?;
            let s = entry.value();
            (
                Utc::now().signed_duration_since(s.token.created_at).num_seconds()
                    <= MAX_SNIPE_AGE_SECS,
                s.first_slot_buy_sol,
                s.first_slot_sell_sol,
            )
        };
        if !fresh {
            return None;
        }
        self.first_slot_emitted.insert(mint.to_string());
        Some(Event::FirstSlotSettled {
            mint: Mint::from(mint),
            buy_lamports: sol_to_lamports_u64(buy_sol),
            sell_lamports: sol_to_lamports_u64(sell_sol),
            at: Utc::now(),
        })
    }

    /// Settle every pending creation slot the watermark has passed. Called once per
    /// clock tick, so a launch resolves within `TICK_MS` of the chain moving on
    /// rather than whenever the token happens to trade again. Entries that can no
    /// longer settle (uncached, or aged past the snipe rail) are dropped so the map
    /// stays bounded by tokens created in the last few slots.
    pub fn settle_ready(&mut self) -> ProducedEvents {
        let mut out = ProducedEvents::new();
        if self.pending_first_slot.is_empty() {
            return out;
        }
        let watermark = self.slot_watermark;
        let ready: Vec<String> = self
            .pending_first_slot
            .iter()
            .filter(|(_, &slot)| watermark > slot)
            .map(|(m, _)| m.clone())
            .collect();
        for mint in ready {
            self.pending_first_slot.remove(&mint);
            if let Some(ev) = self.settle_first_slot(&mint) {
                out.push(ev);
            }
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
        self.trade_cursor.retain(|m, _| keep(m.as_str()));
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
        // Price impact is charged on the PRICED reserve (`vsol`), which is what the
        // reserve pair carries; `real_reserve_sol` is `vsol - 30` on the curve and is
        // the wrong basis. See `TradeLite::priced_reserve_sol`.
        priced_reserve_sol: ct.reserve_sol.unwrap_or(f64::NAN),
        at: ct.block_time,
        // Hashed once at cache ingest (`CachedTrade::from_trade`).
        ix_hash: ct.ix_hash,
        wallet_hash: ct.wallet_hash,
        // The cursor every slot-unit window counts in.
        slot: ct.slot,
        marker_bits: ct.marker_bits,
        // Which instruction of its transaction this is. A bundle selling several
        // wallets' bags emits one trade per leg, all carrying the same `ix_labels`,
        // so `m_dump_ix`'s transaction count reads leg 0 only. Saturates: the byte is
        // only ever compared against 0, and a tx never carries 255 legs.
        leg_index: ct.leg_index.min(u8::MAX as u32) as u8,
        tx_index: Some(ct.tx_index),
        template_hash: ct.template_hash,
        is_launch: ct.is_launch,
        on_curve: ct.on_curve,
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

    /// A freshly created token carrying its creation slot — what the watermark
    /// sweep needs in order to queue a pending settle.
    fn token_at_slot(creation_slot: u64) -> Token {
        let mut t = token(0);
        t.creation_slot = Some(creation_slot);
        t
    }

    fn created_ping() -> StrategyPing {
        StrategyPing { mint: MINT.into(), kind: IngestKind::TokenCreated, received_at: None }
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

    /// The whole point of the watermark: a launch whose token does not trade again
    /// still settles, because some *other* token traded in a later slot. Before
    /// this, the creation slot stayed unresolved until this mint's next trade —
    /// which on a bundled launch is several slots of price movement later.
    #[test]
    fn watermark_settles_a_launch_that_never_trades_again() {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token_at_slot(100));
        state.add_trade(trade(0, 100)); // creation-slot trade only
        cache.insert(MINT.into(), state);
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(30));

        p.on_ping(&created_ping());
        assert!(p.settle_ready().is_empty(), "watermark still at the creation slot");

        p.slot_watermark = 101; // another token traded in the next slot
        let out = p.settle_ready();
        assert_eq!(out.len(), 1, "the chain moved on ⇒ the creation slot is closed");
        assert!(matches!(out[0], Event::FirstSlotSettled { .. }));
    }

    /// Draining any mint's trades advances the watermark — that is what makes the
    /// sweep work without a second feed subscription.
    #[test]
    fn draining_trades_advances_the_watermark() {
        let cache = cache_with(0, vec![trade(0, 7), trade(0, 9)]);
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(30));
        p.on_ping(&ping());
        assert_eq!(p.slot_watermark, 9);
    }

    /// One settle per mint, whichever path gets there first.
    #[test]
    fn watermark_and_trade_paths_never_double_settle() {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token_at_slot(100));
        state.add_trade(trade(0, 100));
        cache.insert(MINT.into(), state);
        let mut p = Producer::new(cache.clone(), Utc::now() - ChronoDuration::seconds(30));

        p.on_ping(&created_ping());
        p.slot_watermark = 101;
        assert_eq!(p.settle_ready().len(), 1);

        // The token finally trades in a later slot: the old path must stay quiet.
        cache.get_mut(MINT).unwrap().value_mut().add_trade(trade(0, 102));
        let out = p.on_ping(&ping());
        assert!(
            !out.events.iter().any(|e| matches!(e, Event::FirstSlotSettled { .. })),
            "already settled by the watermark sweep"
        );
        assert!(p.settle_ready().is_empty(), "and the pending entry is gone");
    }

    /// The restart rail applies to the sweep too: a creation slot that closed hours
    /// ago is not news, however far the watermark has advanced.
    #[test]
    fn watermark_does_not_settle_a_stale_token() {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token(MAX_SNIPE_AGE_SECS + 60));
        state.token.creation_slot = Some(100);
        state.add_trade(trade(MAX_SNIPE_AGE_SECS + 60, 100));
        cache.insert(MINT.into(), state);
        let mut p = Producer::new(cache, Utc::now() - ChronoDuration::seconds(60));

        p.pending_first_slot.insert(MINT.into(), 100);
        p.slot_watermark = 999;
        assert!(p.settle_ready().is_empty(), "stale creation slots stay unsettled");
    }
}
