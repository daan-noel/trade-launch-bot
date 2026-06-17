use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tracing::info;

use crate::config::constants::{
    total_supply_for, DEAD_DUST_VOLUME_SOL, DEAD_DUST_WINDOW_SECONDS, DEAD_MAX_LIQUIDITY_SOL,
    DEAD_MIN_AGE_SECONDS, DEAD_PRICE_PROXIMITY_RATIO, INITIAL_VIRTUAL_TOKEN_RESERVES,
    LIFETIME_GAP_SECONDS, PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN, TOKEN_CACHE_EVICT_IDLE_SECONDS,
    TOKEN_CACHE_EVICT_INTERVAL_SECONDS,
};
use crate::models::token::Token;
use crate::models::trade::{Trade, TradeRow, TradeType};
use crate::sweep::projection::WalletInterner;

/// Hard cap on retained in-memory trade history per token. The cache keeps only
/// the most recent `MAX_TRADES_RETAINED` trades; the oldest are trimmed from the
/// front once the vec exceeds the cap by `TRADES_TRIM_SLACK` (batched so the
/// O(n) front-drain amortizes to O(1) per trade).
///
/// SAFETY — why a fixed cap doesn't corrupt any trade/exit decision: every
/// consumer that walks `trades` either needs only the tail (`active_lifetime_secs`,
/// and the exit re-walk/memo for an open position whose entry is within the
/// window) or treats it as a display sample (`unique_wallets`, swing analysis,
/// the trades API). For the sniper use case a position's whole entry→exit span is
/// a tiny fraction of this window, so the cap never reaches a trade that an open
/// position still needs. The exit memo folds against an *absolute* count
/// (`CachedExitState::consumed_abs`) mapped through `trades_base`, so front-trims
/// can never skip or double-fold a trade. Backtest/paper sims read full history
/// from the DB, not this cache, so they are unaffected.
pub const MAX_TRADES_RETAINED: usize = 50_000;
/// Trim only once the window overruns the cap by this much, so the front-drain
/// runs at most once per `TRADES_TRIM_SLACK` trades instead of on every push.
pub const TRADES_TRIM_SLACK: usize = 5_000;

// ---------------------------------------------------------------------------
// CachedTrade
// ---------------------------------------------------------------------------

/// Slim, cache-resident projection of [`Trade`] for the live token cache.
///
/// `TokenState::trades` retains up to `MAX_TRADES_RETAINED` rows per token across
/// thousands of resident tokens, so the per-row size dominates the live process's
/// heap. This drops every `Trade` field the live hot path never reads off the
/// cache — `id`, `mint_address` (the cache key already identifies the mint),
/// `received_at`, `instruction_labels`, `instruction_type`, `venue`, and
/// `real_token_reserves` — roughly halving the row (~600 B → ~300 B) with no
/// decision-logic change. The full `Trade` is still what the DB and the
/// `/api/tokens/:mint/trades` + swing endpoints serve (both read from Postgres).
///
/// Implements [`TradeRow`] (`Wallet = u32`) so every shared entry/exit/cohort fn
/// runs against it unchanged — exactly as the sweep's `SweepTrade` does. No
/// `tx_signature` is retained (Phase B step 1): the live paper fill resolvers work
/// by **trade index**, not sig match, and `tx_signature()` returns `""` (same as
/// `SweepTrade`) — the backtest still reads the real sig off the DB `Trade`, and
/// real mode records it from the on-chain confirm, never the cache.
///
/// The wallet is stored as a token-local interned `u32` (Phase B step 2), not the
/// 44-byte base58 String: `TokenState::interner` maps `u32 → address` and is the
/// only resident copy of each distinct wallet string. Cohort-set membership in the
/// entry/exit walks is therefore integer-keyed (a hot-path win too).
#[derive(Clone, Debug)]
pub struct CachedTrade {
    /// Token-local interned wallet id (index into `TokenState::interner`'s table).
    pub wallet: u32,
    pub is_buy: bool,
    /// `true` for a bonding-curve leg, `false` for a post-migration AMM leg. A
    /// 1-byte stand-in for the dropped `venue` String — the only `venue` use off
    /// the cache is the sweep corpus's `curve_only` filter (`CacheSource`).
    pub is_curve: bool,
    pub sol_amount: f64,
    pub token_amount: f64,
    pub price_per_token: f64,
    pub slot: u64,
    pub leg_index: u32,
    pub block_time: DateTime<Utc>,
    pub virtual_sol_reserves: Option<f64>,
    pub virtual_token_reserves: Option<f64>,
    pub real_sol_reserves: Option<f64>,
}

impl CachedTrade {
    /// Project a live [`Trade`] into the slim cache row, given the wallet's already-
    /// interned `u32` id (the caller interns against the owning `TokenState`, so this
    /// stays a plain field copy — no `From` impl, since the wallet can't be resolved
    /// without the token-local interner).
    pub fn from_trade(t: &Trade, wallet: u32) -> Self {
        Self {
            wallet,
            is_buy: matches!(t.trade_type, TradeType::Buy),
            is_curve: t.venue == "curve",
            sol_amount: t.sol_amount,
            token_amount: t.token_amount,
            price_per_token: t.price_per_token,
            slot: t.slot,
            leg_index: t.leg_index,
            block_time: t.block_time,
            virtual_sol_reserves: t.virtual_sol_reserves,
            virtual_token_reserves: t.virtual_token_reserves,
            real_sol_reserves: t.real_sol_reserves,
        }
    }
}

impl TradeRow for CachedTrade {
    type Wallet = u32;

    fn is_buy(&self) -> bool {
        self.is_buy
    }
    fn sol_amount(&self) -> f64 {
        self.sol_amount
    }
    fn token_amount(&self) -> f64 {
        self.token_amount
    }
    fn price_per_token(&self) -> f64 {
        self.price_per_token
    }
    fn slot(&self) -> u64 {
        self.slot
    }
    fn leg_index(&self) -> u32 {
        self.leg_index
    }
    fn block_time(&self) -> DateTime<Utc> {
        self.block_time
    }
    fn virtual_sol_reserves(&self) -> Option<f64> {
        self.virtual_sol_reserves
    }
    fn virtual_token_reserves(&self) -> Option<f64> {
        self.virtual_token_reserves
    }
    fn real_sol_reserves(&self) -> Option<f64> {
        self.real_sol_reserves
    }
    fn wallet(&self) -> &u32 {
        &self.wallet
    }
    /// No signature is retained on the cache row (Phase B step 1) — the live paper
    /// resolvers key off the trade index, not the sig. Mirrors `SweepTrade`.
    fn tx_signature(&self) -> &str {
        ""
    }
}

// ---------------------------------------------------------------------------
// TokenState
// ---------------------------------------------------------------------------

/// In-memory state for a single tracked token.
/// Lives inside `TokenCache` for the lifetime of the server process.
#[derive(Clone)]
pub struct TokenState {
    pub token: Token,

    /// Retained trade history in chronological order (oldest first), capped at
    /// `MAX_TRADES_RETAINED`. NOT necessarily the full history — see `trades_base`.
    ///
    /// Wrapped in `Arc` so readers (paper fill-polls, the exit memo seed) clone a
    /// refcount under the shard guard instead of deep-copying up to 50K
    /// `CachedTrade`s while the ingest writer is blocked on `get_mut`. The append
    /// hot path mutates in place via `Arc::make_mut`, which only copies on the rare
    /// tick where a reader is still holding a snapshot — and that copy lands on the
    /// writer, off the read guard.
    pub trades: Arc<Vec<CachedTrade>>,

    /// Absolute count of trades trimmed from the front of `trades` over the
    /// token's lifetime. The logical index of `trades[0]` is `trades_base`, so
    /// `trades_base + trades.len()` is the total ever seen. The exit memo maps its
    /// absolute fold cursor through this to stay correct across front-trims.
    pub trades_base: u64,

    /// Cumulative SOL volume across all trades since tracking began.
    pub volume_sol_total: f64,

    /// Total number of trades seen since tracking began.
    pub trade_count: u64,

    pub last_trade_at: Option<DateTime<Utc>>,
    /// Initial virtual token reserves for circulating supply computation.
    pub initial_virtual_token_reserves: Option<f64>,
    /// Latest virtual token reserves snapshot.
    pub current_virtual_token_reserves: Option<f64>,
    /// Latest known **real** SOL reserves — the dead-token liquidity signal
    /// (`is_dead` Signal 1). Maintained from the chronologically newest trade that
    /// carries a snapshot, NOT `trades.last()`, so a lag-inverted older trade
    /// arriving last can't rewind it. Only advanced by trades that carry reserves,
    /// so a reserve-less newest trade doesn't shadow the last real reading.
    pub current_real_sol_reserves: Option<f64>,
    /// Latest computed market cap (SOL) if available.
    pub market_cap: Option<f64>,
    /// Current token price from the latest trade.
    pub current_price: Option<f64>,
    /// All-time high price observed in trade history.
    pub ath_price: Option<f64>,
    /// Timestamp when ath_price was observed.
    pub ath_timestamp: Option<DateTime<Utc>>,
    /// Whether token has migrated from bonding curve to AMM.
    pub is_migrated: bool,
    /// Set once this token's PumpSwap AMM pool caches have been warmed (or a warm
    /// is in flight). Lives on the token's own state so the "warm once per mint"
    /// guard is bounded by the cache — no separate, never-evicted map.
    pub amm_pool_prewarmed: bool,
    /// Wall-clock time of the last successful manual sync, if any. Populated from
    /// `tokens_info.last_synced_at` on seed and refreshed after each sync.
    pub last_synced_at: Option<DateTime<Utc>>,

    /// Token-local wallet interner: maps each distinct wallet address to a dense
    /// `u32` id (`CachedTrade::wallet`) and back. The only resident copy of each
    /// wallet string for this token (Phase B step 2). Grows with distinct wallets
    /// seen and is **not** trimmed when `trades` front-trims — the `u32` ids in the
    /// retained rows stay valid because table indices never shift.
    pub interner: WalletInterner,
}

impl TokenState {
    pub fn new(token: Token) -> Self {
        let ath_timestamp = Some(token.created_at);
        let initial_price = token
            .initial_buy_sol
            .zip(token.initial_supply_token)
            .and_then(|(buy, supply)| {
                if supply > 0 {
                    Some(buy / supply as f64)
                } else {
                    None
                }
            });

        Self {
            token,
            trades: Arc::new(Vec::new()),
            trades_base: 0,
            volume_sol_total: 0.0,
            trade_count: 0,
            last_trade_at: None,
            initial_virtual_token_reserves: None,
            current_virtual_token_reserves: None,
            current_real_sol_reserves: None,
            market_cap: None,
            current_price: initial_price,
            ath_price: initial_price,
            ath_timestamp,
            is_migrated: false,
            amm_pool_prewarmed: false,
            last_synced_at: None,
            interner: WalletInterner::default(),
        }
    }

    /// Intern a live trade's wallet to this token's `u32` namespace and project it
    /// into the slim cache row. The interning side-effect makes this `&mut self`;
    /// the returned row is then appended via [`push_trade_capped`](Self::push_trade_capped).
    /// Used by [`add_trade`](Self::add_trade) and the seed/sync rebuild paths.
    pub fn intern_trade(&mut self, trade: &Trade) -> CachedTrade {
        let wallet = self.interner.intern(&trade.wallet_address);
        CachedTrade::from_trade(trade, wallet)
    }

    /// Clone this token's `u32 → address` wallet table — for the sweep cache corpus
    /// source, which projects the retained `CachedTrade`s into `SweepTrade`s reusing
    /// these exact ids (no re-hash) and needs the table to write addresses back.
    pub fn wallet_table(&self) -> Vec<Box<str>> {
        self.interner.clone_table()
    }

    /// The token's launch price (initial buy SOL ÷ initial supply), if both are
    /// known. This is the baseline the dead-token price signal compares against.
    pub fn initial_price(&self) -> Option<f64> {
        self.token
            .initial_buy_sol
            .zip(self.token.initial_supply_token)
            .and_then(|(buy, supply)| (supply > 0).then(|| buy / supply as f64))
    }

    /// Whether this token looks **dead** at `now`: nobody cares anymore. True only
    /// when ALL signals hold — liquidity gone, price round-tripped to launch, and
    /// only dust trades in the trailing window — and the token is past
    /// `DEAD_MIN_AGE_SECONDS` so a fresh launch isn't misflagged. Pure + read from
    /// in-memory state (no DB), so it's cheap enough to recompute per flush. See
    /// the `DEAD_*` constants for the rationale on AND-ing the signals.
    pub fn is_dead(&self, now: DateTime<Utc>) -> bool {
        // Age gate — a brand-new mint can transiently satisfy every signal.
        if now.signed_duration_since(self.token.created_at).num_seconds() < DEAD_MIN_AGE_SECONDS {
            return false;
        }

        // Signal 1 — liquidity gone. Latest real SOL reserves below the floor.
        // Reads the maintained newest-by-block_time snapshot (not `trades.last()`,
        // which gRPC index lag can leave pointing at an older trade). No snapshot
        // yet → can't confirm death, treat as alive.
        let latest_liquidity = self.current_real_sol_reserves;
        let liquidity_gone = matches!(latest_liquidity, Some(sol) if sol < DEAD_MAX_LIQUIDITY_SOL);
        if !liquidity_gone {
            return false;
        }

        // Signal 2 — price round-tripped to (or below) launch price. The baseline
        // is the token's own recorded dev-buy fill when known (self-calibrating to
        // its recorded units); otherwise the bonding-curve genesis price — a
        // constant for every Pump.fun mint — so a no-dev-buy mint is still evaluated
        // against a real floor rather than being silently immune to deadness.
        let baseline = self
            .initial_price()
            .unwrap_or(PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN);
        let price_at_launch = match self.current_price {
            Some(price) if baseline > 0.0 => price <= baseline * (1.0 + DEAD_PRICE_PROXIMITY_RATIO),
            _ => false,
        };
        if !price_at_launch {
            return false;
        }

        // Signal 3 — only dust (or nothing) traded in the trailing window. Trades
        // are appended in arrival order, which is *almost* block-time order but can
        // briefly invert under gRPC index lag, so `filter` the whole retained tail
        // rather than `take_while` from the back: a stray out-of-order older trade
        // must not stop the walk early and under-count recent volume (which would
        // risk a false-positive dead flag). Gated behind Signals 1+2, this only
        // runs on already-drained, at-launch-price tokens — a short, low-volume
        // tail — so scanning it in full stays cheap on the hot path.
        let cutoff = now - chrono::Duration::seconds(DEAD_DUST_WINDOW_SECONDS);
        let recent_volume: f64 = self
            .trades
            .iter()
            .filter(|t| t.block_time >= cutoff)
            .map(|t| t.sol_amount)
            .sum();
        recent_volume <= DEAD_DUST_VOLUME_SOL
    }

    /// Append a live trade and update aggregate metrics.
    pub fn add_trade(&mut self, trade: Trade) {
        self.apply_aggregates(&trade);
        let cached = self.intern_trade(&trade);
        self.push_trade_capped(cached);
    }

    /// Re-fold a retained [`CachedTrade`] through the aggregate path. Used by
    /// `recompute_token_state`, the only caller that rebuilds a token's state from
    /// its already-slimmed retained history rather than from live `Trade`s.
    pub fn add_cached_trade(&mut self, trade: CachedTrade) {
        self.apply_aggregates(&trade);
        self.push_trade_capped(trade);
    }

    /// Fold one trade (live `Trade` or retained `CachedTrade` — both `TradeRow`)
    /// into the aggregate metrics: cumulative volume, count, ATH, and the
    /// newest-by-block_time snapshots (price, reserves, market cap).
    fn apply_aggregates<T: TradeRow>(&mut self, trade: &T) {
        self.volume_sol_total += trade.sol_amount();
        self.trade_count += 1;

        let price = trade.price_per_token();

        // All-time high is a max — order-independent, so always considered.
        if self.ath_price.is_none() || price > self.ath_price.unwrap_or(0.0) {
            self.ath_price = Some(price);
            self.ath_timestamp = Some(trade.block_time());
        }

        // "Latest" snapshots — last_trade_at, current_price, reserves, market cap —
        // must track the chronologically newest trade, NOT merely the most recently
        // appended one: gRPC index lag can deliver an older trade after a newer one
        // (the same inversion `is_dead` Signal 3 defends against). Advance them only
        // when this trade is at least as new as the newest seen, so a lag-delayed
        // old trade can't rewind price/liquidity and mis-flag deadness.
        let is_newest = self.last_trade_at.map_or(true, |t| trade.block_time() >= t);
        if is_newest {
            self.last_trade_at = Some(trade.block_time());
            self.current_price = Some(price);
            self.update_reserves(trade);
            self.update_market_cap(price);
        }
    }

    /// Append a trade to the retained history, trimming the oldest trades once the
    /// window overruns the cap by `TRADES_TRIM_SLACK`. Aggregate metrics are NOT
    /// touched here (callers either updated them already in `add_trade` or seed
    /// them from the DB), so this is also the path the cache seed uses to bound
    /// startup memory without recomputing stats. `trades_base` advances by exactly
    /// the number trimmed so the exit memo's absolute cursor stays valid.
    pub fn push_trade_capped(&mut self, trade: CachedTrade) {
        // `make_mut` mutates in place when we hold the only reference (the common
        // hot-path case); it copies once only if an API reader is still holding a
        // snapshot Arc from a concurrent request.
        let trades = Arc::make_mut(&mut self.trades);
        trades.push(trade);
        let len = trades.len();
        if len > MAX_TRADES_RETAINED + TRADES_TRIM_SLACK {
            let overflow = len - MAX_TRADES_RETAINED;
            trades.drain(0..overflow);
            self.trades_base += overflow as u64;
        }
    }

    fn update_reserves<T: TradeRow>(&mut self, trade: &T) {
        if let Some(current) = trade.virtual_token_reserves() {
            // Use the configured static initial virtual token reserves as the
            // baseline. Do not attempt to reconstruct initial reserves from
            // the first trade — it's constant for Pump.fun tokens.
            if self.initial_virtual_token_reserves.is_none() {
                self.initial_virtual_token_reserves = Some(INITIAL_VIRTUAL_TOKEN_RESERVES);
            }
            self.current_virtual_token_reserves = Some(current);
        }
        // Newest known real SOL reserves for the dead-token liquidity signal. Only
        // overwrite when this trade carries a snapshot, so a reserve-less newest
        // trade doesn't shadow the last real reading.
        if let Some(sol) = trade.real_sol_reserves() {
            self.current_real_sol_reserves = Some(sol);
        }
    }

    fn update_market_cap(&mut self, price: f64) {
        // FDV in SOL: total supply × curve spot price (GMGN-style). Mayhem-mode
        // tokens are minted with 2× supply, so scale accordingly.
        let supply = total_supply_for(self.token.is_mayhem_mode);
        self.market_cap = Some(supply * price);
    }

    /// Count unique wallets across the retained trade history. For a token under
    /// `MAX_TRADES_RETAINED` this is exact; for one that has overrun the cap it is
    /// the distinct-wallet count within the retained window (a display metric, so
    /// the windowed approximation is acceptable).
    pub fn unique_wallets(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for t in self.trades.iter() {
            seen.insert(t.wallet);
        }
        seen.len()
    }

    /// Gap-aware active lifetime in seconds: from token creation to the last
    /// "real" trade. Trailing trades separated from their predecessor by a
    /// silence longer than `LIFETIME_GAP_SECONDS` are stripped first, so a lone
    /// late trade after the token went quiet doesn't inflate the lifetime.
    /// `None` when the token has no trades. Trades are stored oldest-first.
    pub fn active_lifetime_secs(&self) -> Option<i64> {
        let trades: &[CachedTrade] = &self.trades;
        let mut end = trades.len();
        if end == 0 {
            return None;
        }
        while end >= 2 {
            let gap = trades[end - 1]
                .block_time
                .signed_duration_since(trades[end - 2].block_time)
                .num_seconds();
            if gap > LIFETIME_GAP_SECONDS {
                end -= 1;
            } else {
                break;
            }
        }
        let death = trades[end - 1].block_time;
        Some(
            death
                .signed_duration_since(self.token.created_at)
                .num_seconds()
                .max(0),
        )
    }
}

// ---------------------------------------------------------------------------
// TokenCache
// ---------------------------------------------------------------------------

/// Concurrent in-memory map: mint_address → TokenState.
/// A token present here means it is actively tracked.
pub type TokenCache = DashMap<String, TokenState>;

// ---------------------------------------------------------------------------
// Runtime eviction
// ---------------------------------------------------------------------------

/// Eviction predicate: a token held by an open position is **never** evictable;
/// otherwise it is evictable when **either**
///   - it is **dead** (`TokenState::is_dead` — liquidity gone, price round-tripped
///     to launch, only dust trading; past `DEAD_MIN_AGE_SECONDS`), regardless of
///     how recently a (dust) trade arrived. A dead token keeps drawing dust trades,
///     so `last_trade_at` stays fresh and the idle check below would never fire —
///     yet it is worthless to track and every dust trade for it is wasted ingest
///     work. Dropping it stops the pipeline from appending its trades (a now-untracked
///     mint is a cache miss), so it is no longer tracked/ingested; **or**
///   - it has been **inactive** for at least `idle_secs`. "Inactive" is measured
///     from the last trade, falling back to the token's creation time for a mint
///     that has never traded — so a freshly-created mint is never dropped before it
///     has had a chance to trade (and be sniped), while a created-but-never-traded
///     launch is still reclaimed once it ages out.
///
/// Pure so the sweep below stays trivially testable without a clock or a `DashMap`.
fn token_is_evictable(state: &TokenState, now: DateTime<Utc>, idle_secs: i64, held: bool) -> bool {
    if held {
        return false;
    }
    // A dead token is reclaimed immediately, even if dust trades keep it "active".
    if state.is_dead(now) {
        return true;
    }
    let last_activity = state.last_trade_at.unwrap_or(state.token.created_at);
    now.signed_duration_since(last_activity).num_seconds() >= idle_secs
}

/// Periodic eviction sweep: drop tokens that hold no open position and are either
/// **dead** (`TokenState::is_dead`) or have gone quiet beyond
/// `TOKEN_CACHE_EVICT_IDLE_SECONDS`, bounding the live process's heap and keeping
/// dead mints out of the ingest hot path. Without this the cache grows one entry
/// per created mint forever (Pump.fun mints thousands/day) → eventual OOM of the
/// trading process.
///
/// `is_held` is supplied by the composition root (closing over the strategy
/// runtime caches' in-memory holding indexes, which cover **both** paper and real
/// positions), so this stays decoupled from `strategies/` and needs no DB round
/// trip — the exemption is read from memory that is already kept in lockstep with
/// every position open/close. An evicted mint is re-added by a fresh manual sync
/// (`token_sync`), matching the seed's activity-window contract.
///
/// Off the hot path: a coarse interval; collect-then-remove so no shard guard is
/// held across the removals and the map is never mutated mid-iteration (mirrors
/// `evict_mayhem_tokens`).
pub async fn run_token_cache_eviction<F>(token_cache: Arc<TokenCache>, is_held: F)
where
    F: Fn(&str) -> bool + Send + 'static,
{
    let mut tick = tokio::time::interval(Duration::from_secs(TOKEN_CACHE_EVICT_INTERVAL_SECONDS));
    // `interval` fires its first tick immediately; skip it so the first sweep
    // never races the still-hydrating startup seed (and so a just-booted process
    // gives live ingest a full interval to populate first).
    tick.tick().await;
    loop {
        tick.tick().await;

        let now = Utc::now();
        let stale: Vec<String> = token_cache
            .iter()
            .filter(|e| {
                token_is_evictable(e.value(), now, TOKEN_CACHE_EVICT_IDLE_SECONDS, is_held(e.key()))
            })
            .map(|e| e.key().clone())
            .collect();

        if stale.is_empty() {
            continue;
        }
        for mint in &stale {
            token_cache.remove(mint);
        }
        info!(
            "TokenCache eviction: dropped {} token(s) (dead or >{}s quiet, no open position); {} remain",
            stale.len(),
            TOKEN_CACHE_EVICT_IDLE_SECONDS,
            token_cache.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn token_created_at(created_at: DateTime<Utc>) -> Token {
        Token::new(
            "MINT-evict".into(),
            "creator".into(),
            "Evict Test".into(),
            "EVT".into(),
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
            created_at,
        )
    }

    const IDLE: i64 = TOKEN_CACHE_EVICT_IDLE_SECONDS;

    #[test]
    fn keeps_recently_traded_token() {
        let now = Utc::now();
        let mut state = TokenState::new(token_created_at(now - ChronoDuration::days(2)));
        state.last_trade_at = Some(now - ChronoDuration::seconds(IDLE / 2));
        assert!(!token_is_evictable(&state, now, IDLE, false));
    }

    #[test]
    fn evicts_long_quiet_token() {
        let now = Utc::now();
        let mut state = TokenState::new(token_created_at(now - ChronoDuration::days(2)));
        state.last_trade_at = Some(now - ChronoDuration::seconds(IDLE + 60));
        assert!(token_is_evictable(&state, now, IDLE, false));
    }

    #[test]
    fn keeps_fresh_never_traded_token() {
        // A brand-new mint with no trades yet must survive long enough to trade.
        let now = Utc::now();
        let state = TokenState::new(token_created_at(now - ChronoDuration::seconds(30)));
        assert!(state.last_trade_at.is_none());
        assert!(!token_is_evictable(&state, now, IDLE, false));
    }

    #[test]
    fn evicts_old_never_traded_token() {
        // Created-but-never-traded launches still age out (fallback to created_at).
        let now = Utc::now();
        let state = TokenState::new(token_created_at(now - ChronoDuration::seconds(IDLE + 60)));
        assert!(token_is_evictable(&state, now, IDLE, false));
    }

    #[test]
    fn never_evicts_a_held_mint() {
        // The open-position exemption overrides idleness regardless of how quiet.
        let now = Utc::now();
        let mut state = TokenState::new(token_created_at(now - ChronoDuration::days(30)));
        state.last_trade_at = Some(now - ChronoDuration::days(10));
        assert!(!token_is_evictable(&state, now, IDLE, true));
    }

    #[test]
    fn evicts_dead_token_despite_recent_dust_trade() {
        // A dead token keeps drawing dust trades, so its `last_trade_at` is recent
        // and the idle check never fires — yet it must still be reclaimed so the
        // ingest pipeline stops tracking it. (Helpers defined below in this module.)
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now), "precondition: token is dead");
        // last_trade_at is 30s ago → NOT idle, but dead ⇒ evictable.
        assert!(token_is_evictable(&state, now, IDLE, false));
    }

    #[test]
    fn never_evicts_held_dead_token() {
        // Deadness never overrides the open-position exemption: an exit loop still
        // needs the cache entry, so a held mint survives even when flagged dead.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now), "precondition: token is dead");
        assert!(!token_is_evictable(&state, now, IDLE, true));
    }

    // ── is_dead ─────────────────────────────────────────────────────────────

    use crate::models::trade::{Trade, TradeType};

    /// Token with a known launch price (initial_buy_sol ÷ initial_supply_token).
    fn token_with_launch(created_at: DateTime<Utc>, buy_sol: f64, supply: u64) -> Token {
        Token::new(
            "MINT-dead".into(),
            "creator".into(),
            "Dead Test".into(),
            "DEAD".into(),
            None,
            None,
            Some(supply),
            Some(buy_sol),
            None,
            None,
            None,
            false,
            false,
            serde_json::Value::Array(vec![]),
            "create-sig".into(),
            created_at,
        )
    }

    fn trade_at(at: DateTime<Utc>, sol: f64, tokens: f64, real_sol_reserves: f64) -> Trade {
        let mut t = Trade::new(
            "MINT-dead".into(),
            "buyer".into(),
            TradeType::Buy,
            sol,
            tokens,
            "sig".into(),
            1,
            at,
        );
        t.real_sol_reserves = Some(real_sol_reserves);
        t
    }

    #[test]
    fn dead_when_all_signals_hold() {
        // launch price = 1.0 / 1000 = 0.001; a dust trade at ≈that price, with
        // liquidity drained below the floor, on a token old enough to evaluate.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now));
    }

    #[test]
    fn alive_when_liquidity_present() {
        // Same dust + price, but reserves still healthy → not dead.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 5.0));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn alive_when_price_well_above_launch() {
        // Liquidity low + dust, but price is 5× launch (0.005 vs 0.001) → still alive.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.005, 1.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn alive_when_recent_volume_above_dust() {
        // Liquidity low + price at launch, but a non-dust trade in the window.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 1.0, 1000.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn fresh_token_never_dead() {
        // Every signal holds, but the token is younger than DEAD_MIN_AGE_SECONDS.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::seconds(30), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(10), 0.001, 1.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn alive_when_no_dev_buy_and_price_above_genesis() {
        // No initial_buy_sol/initial_supply_token → Signal 2 falls back to the
        // genesis floor. This dust trade's price sits far above genesis, so the
        // token has NOT round-tripped to launch and stays alive — even with
        // liquidity drained.
        let now = Utc::now();
        let token = token_created_at(now - ChronoDuration::hours(2)); // no launch fields
        let mut state = TokenState::new(token);
        assert!(state.initial_price().is_none());
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn dead_when_no_dev_buy_and_price_at_genesis_floor() {
        // The fallback's point: a no-dev-buy mint is still evaluable. With price at
        // the genesis floor, liquidity drained, and only dust trading, it is dead —
        // the old None-baseline path would have left it permanently alive.
        let now = Utc::now();
        let token = token_created_at(now - ChronoDuration::hours(2)); // no launch fields
        let mut state = TokenState::new(token);
        assert!(state.initial_price().is_none());
        // price_per_token == genesis floor (sol / tokens == constant / 1.0).
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(30),
            PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN,
            1.0,
            0.5,
        ));
        assert!(state.is_dead(now));
    }

    #[test]
    fn alive_when_recovered_but_stale_trade_appended_last() {
        // Signals 1 & 2 must read the chronologically-newest trade, not the
        // last-*appended* one. A recovered token (healthy reserves, price well
        // above launch) receives an OLD drained, at-launch dust trade last under
        // gRPC index lag. Reading `trades.last()` / a last-write `current_price`
        // would wrongly flag it dead; newest-by-block_time keeps it alive.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Both trades are dust by SOL (so Signal 3 alone reads "quiet" and would
        // flag dead) — only Signals 1 & 2 separate the verdict. Newest by time: a
        // dust buy, but with healthy reserves (5.0) and price (0.5 ≫ launch 0.001).
        state.add_trade(trade_at(now - ChronoDuration::seconds(20), 0.001, 0.002, 5.0));
        // Appended last but timestamped earlier: drained reserves, at-launch price.
        state.add_trade(trade_at(now - ChronoDuration::seconds(120), 0.001, 1.0, 0.2));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn dead_despite_out_of_order_trade_in_window() {
        // Signal 3 must sum the WHOLE in-window tail, not stop at the first
        // out-of-order older trade. Append a recent dust trade, then a trade whose
        // block_time is *earlier* (gRPC index lag inverting arrival order) — both
        // are dust and within the window, so the token is still dead. A `take_while`
        // from the back would stop at the older trade and could misjudge.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.001, 1.0, 0.5));
        // Arrives after, but timestamped earlier — still inside the dust window.
        state.add_trade(trade_at(now - ChronoDuration::seconds(90), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now));
    }

    #[test]
    fn alive_when_out_of_order_trade_pushes_window_volume_over_dust() {
        // The flip side, and the case that actually distinguishes `filter` from
        // `take_while`: a non-dust trade sits inside the window, but the
        // newest-*appended* trade is timestamped *outside* the window (arrival order
        // inverted by index lag). A back-walk (`take_while`) hits the out-of-window
        // trade first and stops, missing the big trade → wrongly flags dead.
        // `filter` scans the whole tail, counts the big trade, and keeps it alive.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // In-window, well above the dust threshold.
        state.add_trade(trade_at(now - ChronoDuration::seconds(60), 1.0, 1000.0, 0.5));
        // Appended last but timestamped beyond DEAD_DUST_WINDOW_SECONDS — gates a
        // back-walk before it can reach the big trade above.
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(DEAD_DUST_WINDOW_SECONDS + 60),
            0.001,
            1.0,
            0.5,
        ));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn cached_trade_matches_trade_on_every_traderow_read() {
        // CachedTrade is a slim projection of Trade; its TradeRow reads (the only
        // way the live entry/exit/cohort fns see a cache row) must be identical to
        // the source Trade's, or decision parity silently breaks. Mirrors the sweep
        // projection's field-for-field guarantee.
        let now = Utc::now();
        let mut t = Trade::new(
            "MINT-parity".into(),
            "wallet-parity".into(),
            TradeType::Sell,
            1.25,
            42_000.0,
            "sig-parity".into(),
            999,
            now,
        );
        t.leg_index = 3;
        t.virtual_sol_reserves = Some(31.0);
        t.virtual_token_reserves = Some(900_000.0);
        t.real_sol_reserves = Some(7.5);
        t.venue = "amm".into();

        // Intern the wallet against a token-local interner (Phase B step 2), exactly
        // as the live append path does, then project.
        let mut interner = WalletInterner::default();
        let wallet_id = interner.intern(&t.wallet_address);
        let c = CachedTrade::from_trade(&t, wallet_id);

        assert_eq!(c.is_buy(), t.is_buy());
        assert_eq!(c.sol_amount(), t.sol_amount());
        assert_eq!(c.token_amount(), t.token_amount());
        assert_eq!(c.price_per_token(), t.price_per_token());
        assert_eq!(c.slot(), t.slot());
        assert_eq!(c.leg_index(), t.leg_index());
        assert_eq!(c.block_time(), t.block_time());
        assert_eq!(c.virtual_sol_reserves(), t.virtual_sol_reserves());
        assert_eq!(c.virtual_token_reserves(), t.virtual_token_reserves());
        assert_eq!(c.real_sol_reserves(), t.real_sol_reserves());
        // The cache wallet is an interned `u32`; the interner table maps it back to
        // the source `Trade`'s wallet address.
        assert_eq!(*c.wallet(), wallet_id);
        assert_eq!(&*interner.clone_table()[wallet_id as usize], t.wallet());
        // The cache row carries no signature (Phase B step 1): `tx_signature()` is
        // always `""`, regardless of the source `Trade`'s sig.
        assert_eq!(c.tx_signature(), "");
        // `is_curve` stands in for the dropped `venue` String (here an AMM leg).
        assert!(!c.is_curve);
    }
}
