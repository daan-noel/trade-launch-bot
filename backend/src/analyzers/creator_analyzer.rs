#![allow(dead_code)]

use serde_json::json;

use crate::{
    models::{
        analysis::{AnalysisResult, CreatorProfile},
        trade::TradeType,
    },
    state::creator_cache::CreatorState,
};

pub const ANALYZER_NAME: &str = "creator_analyzer";

/// Score weights
const W_SELF_TRADE: f64 = 0.30; // creator trades their own token
const W_SERIAL_CREATOR: f64 = 0.25; // many tokens created
const W_RAPID_CHURN: f64 = 0.25; // rapid buy→sell on own token
const W_SELL_DOMINANT: f64 = 0.20; // mostly sells on own tokens (dump)

/// A creator is considered "serial" once they've launched this many tokens.
const SERIAL_CREATOR_THRESHOLD: usize = 3;

/// Rapid churn: sell within N seconds of a buy on the same token.
const CHURN_WINDOW_SECS: i64 = 300; // 5 minutes

/// Stateless creator behaviour analyzer.
///
/// Examines the creator's trade history and token portfolio to detect patterns
/// consistent with pump-and-dump or wash trading:
///
/// - Trading own tokens (potential front-running / price manipulation)
/// - Serial token creation (rug-pull factory)
/// - Rapid buy → sell sequences (churn)
/// - Sell-dominant activity (dump after pump)
pub struct CreatorAnalyzer;

impl CreatorAnalyzer {
    /// Analyse `state` and produce both a per-mint `AnalysisResult` and an
    /// updated `CreatorProfile`.  Pass `None` for `mint_address` when you want
    /// a wallet-level result only (score is still computed; mint is set to
    /// `"global"`).
    pub fn analyze(
        state: &CreatorState,
        mint_address: Option<&str>,
    ) -> (AnalysisResult, CreatorProfile) {
        let mut score = 0.0_f64;
        let mut indicators: Vec<String> = Vec::new();

        let mint = mint_address.unwrap_or("global").to_owned();
        let own_mints: std::collections::HashSet<&str> =
            state.created_tokens.iter().map(String::as_str).collect();

        let history = &state.trade_history;

        // ---------------------------------------------------------------
        // 1. Self-trading — creator has trades on their own token(s)
        // ---------------------------------------------------------------
        let self_trades: Vec<_> = history
            .iter()
            .filter(|t| own_mints.contains(t.mint_address.as_str()))
            .collect();

        if !self_trades.is_empty() {
            score += W_SELF_TRADE;
            indicators.push(format!(
                "self_trading: {} trades on own token(s)",
                self_trades.len()
            ));
        }

        // ---------------------------------------------------------------
        // 2. Serial creator — launched many tokens
        // ---------------------------------------------------------------
        let token_count = state.created_tokens.len();
        if token_count >= SERIAL_CREATOR_THRESHOLD {
            // Each token beyond the threshold adds a small bonus
            let extra = (token_count - SERIAL_CREATOR_THRESHOLD) as f64;
            let contribution = W_SERIAL_CREATOR * (1.0 + extra * 0.1).min(2.0) / 2.0;
            score += contribution;
            indicators.push(format!("serial_creator: {token_count} tokens launched"));
        }

        // ---------------------------------------------------------------
        // 3. Rapid churn — buy own token then sell quickly
        // ---------------------------------------------------------------
        {
            use std::collections::HashMap;
            let mut last_buy_ts: HashMap<&str, i64> = HashMap::new();
            let mut churns = 0u32;

            for t in self_trades.iter() {
                let ts = t.block_time.timestamp();
                let m = t.mint_address.as_str();
                match t.trade_type {
                    TradeType::Buy => {
                        last_buy_ts.insert(m, ts);
                    }
                    TradeType::Sell => {
                        if let Some(&buy_ts) = last_buy_ts.get(m) {
                            if ts - buy_ts <= CHURN_WINDOW_SECS {
                                churns += 1;
                            }
                            last_buy_ts.remove(m);
                        }
                    }
                }
            }

            if churns > 0 {
                let contribution = W_RAPID_CHURN * (churns as f64 / 3.0_f64).min(1.0);
                score += contribution;
                indicators.push(format!(
                    "rapid_churn: {churns} rapid buy→sell on own token(s)"
                ));
            }
        }

        // ---------------------------------------------------------------
        // 4. Sell-dominant activity on own tokens (dump signature)
        // ---------------------------------------------------------------
        let own_buys = self_trades
            .iter()
            .filter(|t| t.trade_type == TradeType::Buy)
            .count();
        let own_sells = self_trades
            .iter()
            .filter(|t| t.trade_type == TradeType::Sell)
            .count();

        if own_sells > own_buys && own_sells >= 3 {
            score += W_SELL_DOMINANT;
            indicators.push(format!(
                "sell_dominant: {own_sells} sells vs {own_buys} buys on own tokens"
            ));
        }

        let score = score.min(1.0);

        // ---------------------------------------------------------------
        // Build AnalysisResult (per-mint view)
        // ---------------------------------------------------------------
        let result = AnalysisResult::new(
            mint.clone(),
            ANALYZER_NAME.to_owned(),
            score,
            indicators.clone(),
        );

        // ---------------------------------------------------------------
        // Build / update CreatorProfile (wallet-level view)
        // ---------------------------------------------------------------
        let total_volume_sol: f64 = history.iter().map(|t| t.sol_amount).sum();

        // Wash-trade / sell-pressure score: fraction of own-token trades that
        // are *sells*.
        //
        // Why not "self_trades / history"?
        //   trade_history only receives trades made BY this creator wallet, and
        //   in practice every such trade is on their own token — making that
        //   ratio always exactly 1 or 0 (binary, useless).
        //
        // Sell-pressure ranges 0 → 1 and captures dump behaviour:
        //   0.0 = creator only bought their own token (or made no trades)
        //   0.5 = equal buys and sells (normal two-way activity)
        //   1.0 = creator only sold their own token (pure dump signal)
        let own_trade_count = own_buys + own_sells;
        let wash_trade_score = if own_trade_count > 0 {
            own_sells as f64 / own_trade_count as f64
        } else {
            0.0
        };

        let profile = CreatorProfile {
            id: uuid::Uuid::new_v4(),
            wallet_address: state.wallet.clone(),
            tokens_created: token_count as i32,
            total_volume_sol,
            suspiciousness_score: score,
            wash_trade_score,
            last_analyzed_at: Some(chrono::Utc::now()),
            indicators: json!({
                "flags": indicators,
                "self_trade_count": self_trades.len(),
                "token_count": token_count,
            }),
        };
        (result, profile)
    }
}
