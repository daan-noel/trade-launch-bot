#![allow(dead_code)]

use std::collections::HashMap;

use crate::{
    models::{analysis::AnalysisResult, trade::TradeType},
    state::token_cache::TokenState,
};

pub const ANALYZER_NAME: &str = "volume_analyzer";

/// Thresholds — tuned conservatively to minimise false positives.
const LOW_WALLET_DIVERSITY_THRESHOLD: f64 = 0.30; // unique / total < this → suspicious
const VOLUME_CONCENTRATION_THRESHOLD: f64 = 0.60; // one wallet > 60 % of volume
const ROUND_TRIP_WINDOW_SECS: i64 = 120; // buy → sell within 2 min
const HIGH_FREQUENCY_WALLET_RATIO: f64 = 0.10; // < 10 % unique wallets with > 20 trades

// Score weights — must sum ≤ 1.0
const W_LOW_DIVERSITY: f64 = 0.25;
const W_CONCENTRATION: f64 = 0.30;
const W_ROUND_TRIPS: f64 = 0.30;
const W_HIGH_FREQUENCY: f64 = 0.15;

/// Stateless wash-trading analyzer.
///
/// Runs on the current sliding-window snapshot inside `TokenState` and returns
/// an `AnalysisResult` with a score in `[0.0, 1.0]`.  A score close to 1.0
/// means the trade pattern looks highly manipulated.
pub struct VolumeAnalyzer;

impl VolumeAnalyzer {
    /// Produce an analysis result for the given token state.
    /// Cheap — no I/O, no allocations beyond the indicator list.
    pub fn analyze(state: &TokenState) -> AnalysisResult {
        let mut score = 0.0_f64;
        let mut indicators: Vec<String> = Vec::new();

        let trades = &state.recent_trades;
        let n = trades.len();

        if n == 0 {
            return AnalysisResult::new(
                state.token.mint_address.clone(),
                ANALYZER_NAME.to_owned(),
                0.0,
                vec![],
            );
        }

        // ---------------------------------------------------------------
        // 1. Wallet diversity — ratio of unique wallets to trade count
        // ---------------------------------------------------------------
        let unique_count = state.unique_wallets_in_window();
        let diversity = unique_count as f64 / n as f64;

        if diversity < LOW_WALLET_DIVERSITY_THRESHOLD {
            score += W_LOW_DIVERSITY;
            indicators.push(format!(
                "low_wallet_diversity: {unique_count} unique / {n} trades \
                 ({:.0}%)",
                diversity * 100.0
            ));
        }

        // ---------------------------------------------------------------
        // 2. Volume concentration — one wallet dominates SOL volume
        // ---------------------------------------------------------------
        let mut volume_by_wallet: HashMap<&str, f64> = HashMap::new();
        for t in trades {
            *volume_by_wallet
                .entry(t.wallet_address.as_str())
                .or_default() += t.sol_amount;
        }
        let total_vol: f64 = volume_by_wallet.values().sum();
        if total_vol > 0.0 {
            let max_vol = volume_by_wallet.values().cloned().fold(0.0_f64, f64::max);
            let concentration = max_vol / total_vol;
            if concentration > VOLUME_CONCENTRATION_THRESHOLD {
                score += W_CONCENTRATION;
                indicators.push(format!(
                    "volume_concentration: top wallet holds {:.0}% of SOL volume",
                    concentration * 100.0
                ));
            }
        }

        // ---------------------------------------------------------------
        // 3. Round-trip detection — same wallet buys then sells within
        //    ROUND_TRIP_WINDOW_SECS
        // ---------------------------------------------------------------
        // Collect last buy time per wallet, then scan for a sell within window
        let mut last_buy_ts: HashMap<&str, i64> = HashMap::new();
        let mut round_trips = 0u32;

        for t in trades {
            let wallet = t.wallet_address.as_str();
            let ts = t.block_time.timestamp();

            match t.trade_type {
                TradeType::Buy => {
                    last_buy_ts.insert(wallet, ts);
                }
                TradeType::Sell => {
                    if let Some(&buy_ts) = last_buy_ts.get(wallet) {
                        if ts - buy_ts <= ROUND_TRIP_WINDOW_SECS {
                            round_trips += 1;
                        }
                        last_buy_ts.remove(wallet);
                    }
                }
            }
        }

        if round_trips > 0 {
            // Scale: 1 round-trip → small contribution, 5+ → full weight
            let rt_weight = W_ROUND_TRIPS * (round_trips as f64 / 5.0_f64).min(1.0);
            score += rt_weight;
            indicators.push(format!(
                "round_trips: {round_trips} buy→sell within \
                 {ROUND_TRIP_WINDOW_SECS}s window"
            ));
        }

        // ---------------------------------------------------------------
        // 4. High-frequency / low-diversity extreme case
        // ---------------------------------------------------------------
        if unique_count > 0
            && (unique_count as f64 / state.trade_count as f64) < HIGH_FREQUENCY_WALLET_RATIO
            && state.trade_count > 20
        {
            score += W_HIGH_FREQUENCY;
            indicators.push(format!(
                "high_frequency_low_diversity: {} total trades, only \
                 {unique_count} unique wallets",
                state.trade_count
            ));
        }

        let score = score.min(1.0);
        AnalysisResult::new(
            state.token.mint_address.clone(),
            ANALYZER_NAME.to_owned(),
            score,
            indicators,
        )
    }
}
