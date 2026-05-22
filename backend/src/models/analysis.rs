use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Result produced by a single analyzer run on a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub id: Uuid,
    pub mint_address: String,
    /// Identifies which analyzer produced this (e.g. "volume_analyzer").
    pub analyzer_name: String,
    /// Suspiciousness score: 0.0 (clean) → 1.0 (highly suspicious).
    pub score: f64,
    /// Human-readable indicator labels that contributed to the score.
    pub indicators: Vec<String>,
    pub computed_at: DateTime<Utc>,
}

impl AnalysisResult {
    pub fn new(
        mint_address: String,
        analyzer_name: String,
        score: f64,
        indicators: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            mint_address,
            analyzer_name,
            score,
            indicators,
            computed_at: Utc::now(),
        }
    }
}

/// Aggregated reputation profile for a creator wallet.
/// Upserted each time the creator analyzer runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorProfile {
    pub id: Uuid,
    pub wallet_address: String,
    pub tokens_created: i32,
    pub total_volume_sol: f64,
    /// Combined suspiciousness score across all tokens by this creator.
    pub suspiciousness_score: f64,
    pub wash_trade_score: f64,
    pub last_analyzed_at: Option<DateTime<Utc>>,
    /// Flexible JSON bag for additional indicator data.
    pub indicators: Value,
}

impl CreatorProfile {
    #[allow(dead_code)]
    pub fn new(wallet_address: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            wallet_address,
            tokens_created: 0,
            total_volume_sol: 0.0,
            suspiciousness_score: 0.0,
            wash_trade_score: 0.0,
            last_analyzed_at: None,
            indicators: Value::Object(Default::default()),
        }
    }
}
