use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use crate::models::trade::Trade;

/// Maximum number of trades tracked per creator across all their tokens.
pub const CREATOR_TRADE_WINDOW: usize = 100;

// ---------------------------------------------------------------------------
// CreatorState
// ---------------------------------------------------------------------------

/// In-memory profile for a creator wallet.
/// Updated as token creations and trades arrive.
pub struct CreatorState {
    pub wallet: String,

    /// Mint addresses of tokens created by this wallet (this session).
    pub created_tokens: Vec<String>,

    /// Sliding window of trades made by this wallet across all tracked tokens.
    pub trade_history: VecDeque<Trade>,

    /// Current suspiciousness score; updated by the analyzer layer.
    pub suspiciousness_score: f64,

    pub last_updated: DateTime<Utc>,
}

impl CreatorState {
    pub fn new(wallet: String) -> Self {
        Self {
            wallet,
            created_tokens: Vec::new(),
            trade_history: VecDeque::with_capacity(CREATOR_TRADE_WINDOW),
            suspiciousness_score: 0.0,
            last_updated: Utc::now(),
        }
    }

    pub fn add_token(&mut self, mint: String) {
        if !self.created_tokens.contains(&mint) {
            self.created_tokens.push(mint);
        }
        self.last_updated = Utc::now();
    }

    pub fn add_trade(&mut self, trade: Trade) {
        if self.trade_history.len() >= CREATOR_TRADE_WINDOW {
            self.trade_history.pop_front();
        }
        self.trade_history.push_back(trade);
        self.last_updated = Utc::now();
    }

    pub fn update_score(&mut self, score: f64) {
        self.suspiciousness_score = score;
        self.last_updated = Utc::now();
    }
}

// ---------------------------------------------------------------------------
// CreatorCache
// ---------------------------------------------------------------------------

/// Concurrent in-memory map: wallet_address → CreatorState.
pub type CreatorCache = DashMap<String, CreatorState>;
