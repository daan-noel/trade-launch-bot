use chrono::{DateTime, Utc};
use dashmap::DashMap;

use crate::config::constants::{
    total_supply_for, INITIAL_VIRTUAL_TOKEN_RESERVES, LIFETIME_GAP_SECONDS,
};
use crate::models::{token::Token, trade::Trade};

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
// TokenState
// ---------------------------------------------------------------------------

/// In-memory state for a single tracked token.
/// Lives inside `TokenCache` for the lifetime of the server process.
#[derive(Clone)]
pub struct TokenState {
    pub token: Token,

    /// Retained trade history in chronological order (oldest first), capped at
    /// `MAX_TRADES_RETAINED`. NOT necessarily the full history — see `trades_base`.
    pub trades: Vec<Trade>,

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
    /// Wall-clock time of the last successful manual sync, if any. Populated from
    /// `tokens_info.last_synced_at` on seed and refreshed after each sync.
    pub last_synced_at: Option<DateTime<Utc>>,
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
            trades: Vec::new(),
            trades_base: 0,
            volume_sol_total: 0.0,
            trade_count: 0,
            last_trade_at: None,
            initial_virtual_token_reserves: None,
            current_virtual_token_reserves: None,
            market_cap: None,
            current_price: initial_price,
            ath_price: initial_price,
            ath_timestamp,
            is_migrated: false,
            last_synced_at: None,
        }
    }

    /// Append a trade and update aggregate metrics.
    pub fn add_trade(&mut self, trade: Trade) {
        self.volume_sol_total += trade.sol_amount;
        self.trade_count += 1;
        self.last_trade_at = Some(trade.block_time);

        // Update current price from the latest trade
        let price = trade.price_per_token;
        self.current_price = Some(price);

        // Update all-time high price
        if self.ath_price.is_none() || price > self.ath_price.unwrap_or(0.0) {
            self.ath_price = Some(price);
            self.ath_timestamp = Some(trade.block_time);
        }

        self.update_reserves(&trade);
        self.update_market_cap(price);

        self.push_trade_capped(trade);
    }

    /// Append a trade to the retained history, trimming the oldest trades once the
    /// window overruns the cap by `TRADES_TRIM_SLACK`. Aggregate metrics are NOT
    /// touched here (callers either updated them already in `add_trade` or seed
    /// them from the DB), so this is also the path the cache seed uses to bound
    /// startup memory without recomputing stats. `trades_base` advances by exactly
    /// the number trimmed so the exit memo's absolute cursor stays valid.
    pub fn push_trade_capped(&mut self, trade: Trade) {
        self.trades.push(trade);
        let len = self.trades.len();
        if len > MAX_TRADES_RETAINED + TRADES_TRIM_SLACK {
            let overflow = len - MAX_TRADES_RETAINED;
            self.trades.drain(0..overflow);
            self.trades_base += overflow as u64;
        }
    }

    fn update_reserves(&mut self, trade: &Trade) {
        if let Some(current) = trade.virtual_token_reserves {
            // Use the configured static initial virtual token reserves as the
            // baseline. Do not attempt to reconstruct initial reserves from
            // the first trade — it's constant for Pump.fun tokens.
            if self.initial_virtual_token_reserves.is_none() {
                self.initial_virtual_token_reserves = Some(INITIAL_VIRTUAL_TOKEN_RESERVES);
            }
            self.current_virtual_token_reserves = Some(current);
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
        for t in &self.trades {
            seen.insert(t.wallet_address.as_str());
        }
        seen.len()
    }

    /// Gap-aware active lifetime in seconds: from token creation to the last
    /// "real" trade. Trailing trades separated from their predecessor by a
    /// silence longer than `LIFETIME_GAP_SECONDS` are stripped first, so a lone
    /// late trade after the token went quiet doesn't inflate the lifetime.
    /// `None` when the token has no trades. Trades are stored oldest-first.
    pub fn active_lifetime_secs(&self) -> Option<i64> {
        let trades = &self.trades;
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
