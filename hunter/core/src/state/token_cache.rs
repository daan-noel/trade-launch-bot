use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tracing::{info, warn};

use crate::config::constants::{
    market_cap_sol, DEAD_MAX_LIQUIDITY_SOL, DEAD_MEANINGFUL_TRADE_SOL, DEAD_QUIET_SECS,
    INITIAL_VIRTUAL_TOKEN_RESERVES, TOKEN_CACHE_EVICT_IDLE_SECONDS,
    TOKEN_CACHE_EVICT_INTERVAL_SECONDS,
};
use crate::storage::repositories::token_info_repo::TokenInfoRepo;
use crate::models::token::Token;
use crate::models::trade::{Trade, TradeRow, TradeType};
use crate::wallet_interner::WalletInterner;

/// Hard cap on retained in-memory trade history per token. Defined in
/// `config::constants` (single-source with the seed cap) and re-exported here so
/// the long-standing `state::token_cache::MAX_TRADES_RETAINED` path keeps working.
pub use crate::config::constants::MAX_TRADES_RETAINED;
/// Trim only once the window overruns the cap by this much, so the front-drain
/// runs at most once per `TRADES_TRIM_SLACK` trades instead of on every push.
pub const TRADES_TRIM_SLACK: usize = 1_000;

/// The pure dead-token decision, single-sourced so live ([`TokenState::is_dead`])
/// and the analysis death-close ([`crate::strategies::death`]) can never drift.
///
/// A token is dead at `now` when BOTH hold:
///   1. `reserves` (newest real SOL reserves) `< DEAD_MAX_LIQUIDITY_SOL` — liquidity
///      gone. `None` (no reserve snapshot yet) ⇒ alive.
///   2. no meaningful trade for at least `DEAD_QUIET_SECS` — `last_meaningful` is the
///      newest `amount_sol >= DEAD_MEANINGFUL_TRADE_SOL` trade time (callers fall back
///      to `created_at` when none has arrived).
pub fn is_dead_verdict(
    reserves: Option<f64>,
    last_meaningful: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    // Signal 1: liquidity depleted. None → no reserve snapshot yet → alive.
    let reserves_depleted = matches!(reserves, Some(sol) if sol < DEAD_MAX_LIQUIDITY_SOL);
    if !reserves_depleted {
        return false;
    }
    // Signal 2: silent for DEAD_QUIET_SECS (dust ignored by the caller's clock).
    now.signed_duration_since(last_meaningful).num_seconds() >= DEAD_QUIET_SECS
}

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
/// Implements [`TradeRow`] (`Wallet = u32`) so every shared entry/exit fn runs
/// against it unchanged — exactly as the sweep's `CorpusTrade` does. No
/// `tx_signature` is retained (Phase B step 1): the live paper fill resolvers work
/// by **trade index**, not sig match, and `tx_signature()` returns `""` (same as
/// `CorpusTrade`) — the backtest still reads the real sig off the DB `Trade`, and
/// real mode records it from the on-chain confirm, never the cache.
///
/// The wallet is stored as a token-local interned `u32` (Phase B step 2), not the
/// 44-byte base58 String: `TokenState::interner` maps `u32 → address` and is the
/// only resident copy of each distinct wallet string.
#[derive(Clone, Debug)]
pub struct CachedTrade {
    /// Token-local interned wallet id (index into `TokenState::interner`'s table).
    pub wallet: u32,
    pub is_buy: bool,
    pub amount_sol: f64,
    pub token_amount: f64,
    pub price_per_token: f64,
    pub slot: u64,
    pub leg_index: u32,
    pub block_time: DateTime<Utc>,
    pub reserve_sol: Option<f64>,
    pub reserve_token: Option<f64>,
    pub real_reserve_sol: Option<f64>,
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
            amount_sol: t.amount_sol,
            // The cache row keeps token amounts/reserves as `f64` — it's the
            // short-lived hot-path projection feeding ratio math (spot price), not
            // storage. Exact integers live in the `Trade` model + DB; we cast at
            // this boundary.
            token_amount: t.token_amount as f64,
            price_per_token: t.price_per_token,
            slot: t.slot,
            leg_index: t.leg_index,
            block_time: t.block_time,
            reserve_sol: t.reserve_sol,
            reserve_token: t.reserve_token.map(|v| v as f64),
            real_reserve_sol: t.real_reserve_sol,
        }
    }
}

impl TradeRow for CachedTrade {
    type Wallet = u32;

    fn is_buy(&self) -> bool {
        self.is_buy
    }
    fn amount_sol(&self) -> f64 {
        self.amount_sol
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
    fn reserve_sol(&self) -> Option<f64> {
        self.reserve_sol
    }
    fn reserve_token(&self) -> Option<f64> {
        self.reserve_token
    }
    fn real_reserve_sol(&self) -> Option<f64> {
        self.real_reserve_sol
    }
    fn wallet(&self) -> &u32 {
        &self.wallet
    }
    /// No signature is retained on the cache row (Phase B step 1) — the live paper
    /// resolvers key off the trade index, not the sig. Mirrors `CorpusTrade`.
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

    /// Cumulative buy SOL across trades landing in the token's **creation slot**
    /// (`token.creation_slot`). Human SOL in memory (same convention as
    /// `volume_sol_total`); lamports conversion happens at the repo boundary.
    /// Order-independent (a sum) — a same-slot trade delivered late still lands
    /// correctly, as long as `first_slot_window_open` has not closed.
    pub first_slot_buy_sol: f64,
    /// Cumulative sell SOL across creation-slot trades. Same semantics as
    /// `first_slot_buy_sol`.
    pub first_slot_sell_sol: f64,
    /// Latch: once a trade with `slot > creation_slot` is observed the window
    /// closes and same-slot accumulation stops permanently. Not required for
    /// correctness (summing same-slot trades is idempotent), only a cheap early
    /// return so long-lived tokens stop re-checking the condition every trade.
    pub first_slot_window_open: bool,

    pub last_trade_at: Option<DateTime<Utc>>,
    /// Timestamp of the last trade with `amount_sol >= DEAD_MEANINGFUL_TRADE_SOL`.
    /// Dust/probe transactions (below the threshold) do not advance this field so
    /// they cannot keep a dead token alive. Falls back to `token.created_at` in the
    /// `is_dead` check when None (no meaningful trade has ever arrived).
    pub last_meaningful_trade_at: Option<DateTime<Utc>>,
    /// Initial virtual token reserves for circulating supply computation. Seeded
    /// from the on-chain launch constant `INITIAL_VIRTUAL_TOKEN_RESERVES`; this is
    /// genuinely the curve's *virtual* baseline (supply math), distinct from the
    /// venue-neutral `current_reserve_*` price pair below.
    pub initial_virtual_token_reserves: Option<f64>,
    /// Latest token-reserve snapshot from the priced reserve pair (`reserve_token`).
    pub current_reserve_token: Option<f64>,
    /// Latest **SOL**-reserve snapshot from the priced reserve pair (`reserve_sol`,
    /// in SOL — lamports ÷ 1e9 as the decoder stores it). Maintained newest-by-
    /// block_time alongside `current_reserve_token` so the strategy snipe buy can
    /// derive a slippage `min_out` from the in-memory spot price without an inline RPC.
    pub current_reserve_sol: Option<f64>,
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
            first_slot_buy_sol: 0.0,
            first_slot_sell_sol: 0.0,
            first_slot_window_open: true,
            last_trade_at: None,
            last_meaningful_trade_at: None,
            initial_virtual_token_reserves: None,
            current_reserve_token: None,
            current_reserve_sol: None,
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

    /// Whether this token looks **dead** at `now`: nobody cares anymore.
    ///
    /// True when BOTH hold simultaneously:
    ///   1. `current_real_sol_reserves < DEAD_MAX_LIQUIDITY_SOL` — liquidity gone.
    ///   2. No meaningful trade (≥ `DEAD_MEANINGFUL_TRADE_SOL`) for at least
    ///      `DEAD_QUIET_SECS`. Falls back to `token.created_at` when no meaningful
    ///      trade has ever arrived — a token that never got real interest is dead
    ///      once it has been quiet long enough.
    ///
    /// This design is **durable**: a trough (reserves dip, then recover) resets the
    /// quiet clock via the new meaningful trade, so the verdict never flips to true
    /// prematurely. Once both conditions hold, the token stays dead.
    /// Pure + reads only in-memory state; cheap enough to recompute per flush.
    pub fn is_dead(&self, now: DateTime<Utc>) -> bool {
        let last_meaningful = self
            .last_meaningful_trade_at
            .unwrap_or(self.token.created_at);
        is_dead_verdict(self.current_real_sol_reserves, last_meaningful, now)
    }

    /// Seconds from token creation to the last meaningful trade. `Some` only when
    /// the token is dead at `now`; `None` while still alive. Pass the same `now`
    /// used for `is_dead` to avoid a double `Utc::now()` call.
    pub fn lifetime_secs(&self, now: DateTime<Utc>) -> Option<i64> {
        if !self.is_dead(now) {
            return None;
        }
        self.last_meaningful_trade_at.map(|t| {
            t.signed_duration_since(self.token.created_at)
                .num_seconds()
                .max(0)
        })
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
        self.volume_sol_total += trade.amount_sol();
        self.trade_count += 1;

        // Same-slot (creation-slot) buy/sell SOL sums — a derived hot-metric,
        // order-independent by construction. The `first_slot_window_open` latch
        // closes once a later-slot trade is seen so long-lived tokens stop
        // re-checking. A same-slot trade arriving after the window closed
        // under-counts (accepted; matches `ath`/`is_dead` gRPC-reorder tolerance).
        if self.first_slot_window_open {
            match self.token.creation_slot {
                Some(creation_slot) if trade.slot() == creation_slot => {
                    if trade.is_buy() {
                        self.first_slot_buy_sol += trade.amount_sol();
                    } else {
                        self.first_slot_sell_sol += trade.amount_sol();
                    }
                }
                Some(creation_slot) if trade.slot() > creation_slot => {
                    self.first_slot_window_open = false;
                }
                _ => {}
            }
        }

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
            // Only advance the meaningful-trade clock for non-dust trades so bot
            // probe transactions can't keep a dead token alive indefinitely.
            if trade.amount_sol() >= DEAD_MEANINGFUL_TRADE_SOL {
                self.last_meaningful_trade_at = Some(trade.block_time());
            }
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
        if let Some(current) = trade.reserve_token() {
            // Use the configured static initial virtual token reserves as the
            // baseline. Do not attempt to reconstruct initial reserves from
            // the first trade — it's constant for Pump.fun tokens.
            if self.initial_virtual_token_reserves.is_none() {
                self.initial_virtual_token_reserves = Some(INITIAL_VIRTUAL_TOKEN_RESERVES);
            }
            self.current_reserve_token = Some(current);
        }
        // Newest SOL reserves, maintained in lockstep with the token side above
        // (same newest-by-block_time guard) so the snipe buy's slippage min_out
        // reads a consistent (token, sol) reserve pair from memory.
        if let Some(sol) = trade.reserve_sol() {
            self.current_reserve_sol = Some(sol);
        }
        // Newest known real SOL reserves for the dead-token liquidity signal. Only
        // overwrite when this trade carries a snapshot, so a reserve-less newest
        // trade doesn't shadow the last real reading.
        if let Some(sol) = trade.real_reserve_sol() {
            self.current_real_sol_reserves = Some(sol);
        }
    }

    fn update_market_cap(&mut self, price: f64) {
        // FDV in SOL = curve spot price (GMGN-style) × total supply (mayhem-aware),
        // matching the SQL/enrichment canonical (`current_price × total_supply_token`).
        self.market_cap = Some(market_cap_sol(price, self.token.is_mayhem_mode));
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
///   - it is **dead** (`TokenState::is_dead` — reserves depleted + silent for
///     `DEAD_QUIET_SECS` counting only meaningful trades), regardless of how recently
///     a dust trade arrived. A dead token keeps drawing dust trades,
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
pub async fn run_token_cache_eviction<F>(
    token_cache: Arc<TokenCache>,
    is_held: F,
    info_repo: TokenInfoRepo,
)
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

        // Flush the final is_dead=true + lifetime_secs verdict for dead tokens
        // before removing them. The last trade-triggered metrics write had
        // is_dead=false (quiet period not yet elapsed), so without this flush the
        // DB would never see the authoritative dead verdict.
        for mint in &stale {
            if let Some(state) = token_cache.get(mint) {
                if state.is_dead(now) {
                    let age = now
                        .signed_duration_since(state.token.created_at)
                        .num_seconds();
                    let lifetime = state.lifetime_secs(now);
                    if let Err(e) = info_repo
                        .upsert_metrics(
                            mint,
                            state.ath_price,
                            state.ath_timestamp,
                            Some(age),
                            state.volume_sol_total,
                            state.market_cap,
                            state.trade_count as i64,
                            state.last_trade_at,
                            state.current_price,
                            true,
                            state.is_migrated,
                            lifetime,
                            state.first_slot_buy_sol,
                            state.first_slot_sell_sol,
                        )
                        .await
                    {
                        warn!("TokenCache eviction: metrics flush for {mint}: {e}");
                    }
                }
            }
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
            None,
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
            None,
            created_at,
        )
    }

    fn trade_at(at: DateTime<Utc>, sol: f64, tokens: f64, real_reserve_sol: f64) -> Trade {
        let mut t = Trade::new(
            "MINT-dead".into(),
            "buyer".into(),
            TradeType::Buy,
            sol,
            tokens as u64,
            "sig".into(),
            1,
            at,
        );
        t.real_reserve_sol = Some(real_reserve_sol);
        t
    }

    #[test]
    fn dead_when_reserves_low_and_quiet() {
        // Reserves depleted + no meaningful trade for > DEAD_QUIET_SECS → dead.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Dust trade (sol=0.001 < DEAD_MEANINGFUL_TRADE_SOL): does not reset quiet clock.
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(DEAD_QUIET_SECS + 60),
            0.001, 1.0, 0.5,
        ));
        assert!(state.is_dead(now));
    }

    #[test]
    fn alive_when_liquidity_present() {
        // Reserves healthy (>= DEAD_MAX_LIQUIDITY_SOL) → Signal 1 fails → not dead
        // regardless of quiet.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(DEAD_QUIET_SECS + 60), 0.001, 1.0, 50.0));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn alive_when_recently_meaningful_traded() {
        // Reserves low, but a meaningful trade arrived recently (< DEAD_QUIET_SECS ago).
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 1.0, 1000.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn dust_trade_does_not_prevent_death() {
        // A recent dust trade does NOT reset last_meaningful_trade_at, so the token
        // is still dead when the meaningful-trade clock has been quiet long enough.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Old meaningful trade — establishes last_meaningful_trade_at.
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(DEAD_QUIET_SECS + 60),
            0.5, 500.0, 0.5,
        ));
        // Very recent dust trade — must NOT reset the quiet clock.
        state.add_trade(trade_at(now - ChronoDuration::seconds(5), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now));
    }

    #[test]
    fn meaningful_trade_resets_quiet_clock() {
        // A meaningful trade after reserves dip resets the clock → alive (trough recovery).
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Early meaningful trade (would be quiet enough on its own).
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(DEAD_QUIET_SECS + 120),
            0.5, 500.0, 0.5,
        ));
        // Recent meaningful trade — resets clock; token NOT quiet enough → alive.
        state.add_trade(trade_at(now - ChronoDuration::seconds(30), 0.2, 200.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn not_dead_when_no_meaningful_trades_but_young() {
        // No meaningful trades → falls back to created_at. Token is young (< DEAD_QUIET_SECS).
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now - ChronoDuration::seconds(30), 1.0, 1000));
        state.add_trade(trade_at(now - ChronoDuration::seconds(10), 0.001, 1.0, 0.5));
        assert!(!state.is_dead(now));
    }

    #[test]
    fn dead_when_no_meaningful_trades_and_old() {
        // No meaningful trades + token older than DEAD_QUIET_SECS + low reserves → dead.
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(
            now - ChronoDuration::seconds(DEAD_QUIET_SECS + 60), 1.0, 1000,
        ));
        // Only dust trades — reserves low, no meaningful trade ever.
        state.add_trade(trade_at(now - ChronoDuration::seconds(10), 0.001, 1.0, 0.5));
        assert!(state.is_dead(now));
    }

    #[test]
    fn alive_when_newest_reserves_ok_despite_old_stale_trade() {
        // current_real_sol_reserves tracks newest-by-block_time, NOT last-appended.
        // An old stale trade (lower reserves) appended after a newer healthy one
        // must not overwrite the reserves snapshot.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Newest by time: healthy reserves (above DEAD_MAX_LIQUIDITY_SOL), dust sol.
        state.add_trade(trade_at(now - ChronoDuration::seconds(20), 0.001, 0.002, 50.0));
        // Appended later but timestamped earlier: low reserves — must not win.
        state.add_trade(trade_at(now - ChronoDuration::seconds(120), 0.001, 1.0, 0.2));
        // current_real_sol_reserves stays at 50.0 (>= threshold) → not dead.
        assert!(!state.is_dead(now));
    }

    #[test]
    fn alive_when_meaningful_trade_recent_despite_old_dust_trade() {
        // A meaningful trade sets last_meaningful_trade_at to recent. A later-appended
        // but older dust trade is not `is_newest`, so it cannot overwrite that field.
        let now = Utc::now();
        let mut state =
            TokenState::new(token_with_launch(now - ChronoDuration::hours(2), 1.0, 1000));
        // Newer, meaningful: sets last_meaningful_trade_at = now-60s.
        state.add_trade(trade_at(now - ChronoDuration::seconds(60), 1.0, 1000.0, 0.5));
        // Older, appended second: not is_newest, no effect on meaningful clock.
        state.add_trade(trade_at(
            now - ChronoDuration::seconds(DEAD_QUIET_SECS + 60),
            0.001, 1.0, 0.5,
        ));
        // 60s < DEAD_QUIET_SECS → not dead.
        assert!(!state.is_dead(now));
    }

    // ── first-slot buy/sell SOL ───────────────────────────────────────────────

    /// Token whose creation landed in a known slot.
    fn token_at_slot(created_at: DateTime<Utc>, creation_slot: u64) -> Token {
        Token::new(
            "MINT-slot".into(),
            "creator".into(),
            "Slot Test".into(),
            "SLOT".into(),
            None, None, None, None, None, None, None,
            false, false,
            serde_json::Value::Array(vec![]),
            "create-sig".into(),
            Some(creation_slot),
            created_at,
        )
    }

    /// Trade with an explicit slot + direction (creation-slot activity is keyed on
    /// `slot` and `is_buy`, not on price/reserves).
    fn trade_slot(at: DateTime<Utc>, ty: TradeType, sol: f64, slot: u64) -> Trade {
        Trade::new(
            "MINT-slot".into(),
            "buyer".into(),
            ty,
            sol,
            1_000,
            "sig".into(),
            slot,
            at,
        )
    }

    #[test]
    fn accumulates_first_slot_buy_and_sell_sol() {
        let now = Utc::now();
        let mut state = TokenState::new(token_at_slot(now, 100));

        // Two buys + one sell in the creation slot (100).
        state.add_trade(trade_slot(now, TradeType::Buy, 1.5, 100));
        state.add_trade(trade_slot(now, TradeType::Buy, 0.5, 100));
        state.add_trade(trade_slot(now, TradeType::Sell, 0.75, 100));

        assert_eq!(state.first_slot_buy_sol, 2.0);
        assert_eq!(state.first_slot_sell_sol, 0.75);
        assert!(state.first_slot_window_open, "window still open (no later slot yet)");

        // A later-slot trade closes the window and does NOT affect the sums.
        state.add_trade(trade_slot(now, TradeType::Buy, 9.0, 101));
        assert!(!state.first_slot_window_open, "later slot closes the window");
        assert_eq!(state.first_slot_buy_sol, 2.0, "later-slot buy excluded");

        // A same-slot trade arriving AFTER the window closed under-counts (accepted):
        // it must NOT change the frozen sums.
        state.add_trade(trade_slot(now, TradeType::Buy, 3.0, 100));
        assert_eq!(state.first_slot_buy_sol, 2.0, "window closed → same-slot late trade ignored");
    }

    #[test]
    fn first_slot_sums_zero_without_creation_slot() {
        // A token with no known creation_slot never accumulates (None arm).
        let now = Utc::now();
        let mut state = TokenState::new(token_with_launch(now, 1.0, 1000));
        assert!(state.token.creation_slot.is_none());
        state.add_trade(trade_slot(now, TradeType::Buy, 1.0, 1));
        assert_eq!(state.first_slot_buy_sol, 0.0);
        assert_eq!(state.first_slot_sell_sol, 0.0);
    }

    #[test]
    fn cached_trade_matches_trade_on_every_traderow_read() {
        // CachedTrade is a slim projection of Trade; its TradeRow reads (the only
        // way the live entry/exit fns see a cache row) must be identical to the
        // source Trade's, or decision parity silently breaks. Mirrors the sweep
        // projection's field-for-field guarantee.
        let now = Utc::now();
        let mut t = Trade::new(
            "MINT-parity".into(),
            "wallet-parity".into(),
            TradeType::Sell,
            1.25,
            42_000,
            "sig-parity".into(),
            999,
            now,
        );
        t.leg_index = 3;
        t.reserve_sol = Some(31.0);
        t.reserve_token = Some(900_000);
        t.real_reserve_sol = Some(7.5);
        t.venue = "amm".into();

        // Intern the wallet against a token-local interner (Phase B step 2), exactly
        // as the live append path does, then project.
        let mut interner = WalletInterner::default();
        let wallet_id = interner.intern(&t.wallet_address);
        let c = CachedTrade::from_trade(&t, wallet_id);

        assert_eq!(c.is_buy(), t.is_buy());
        assert_eq!(c.amount_sol(), t.amount_sol());
        assert_eq!(c.token_amount(), t.token_amount());
        assert_eq!(c.price_per_token(), t.price_per_token());
        assert_eq!(c.slot(), t.slot());
        assert_eq!(c.leg_index(), t.leg_index());
        assert_eq!(c.block_time(), t.block_time());
        assert_eq!(c.reserve_sol(), t.reserve_sol());
        assert_eq!(c.reserve_token(), t.reserve_token());
        assert_eq!(c.real_reserve_sol(), t.real_reserve_sol());
        // The cache wallet is an interned `u32`; the interner table maps it back to
        // the source `Trade`'s wallet address.
        assert_eq!(*c.wallet(), wallet_id);
        assert_eq!(&*interner.into_table()[wallet_id as usize], t.wallet());
        // The cache row carries no signature (Phase B step 1): `tx_signature()` is
        // always `""`, regardless of the source `Trade`'s sig.
        assert_eq!(c.tx_signature(), "");
    }
}
