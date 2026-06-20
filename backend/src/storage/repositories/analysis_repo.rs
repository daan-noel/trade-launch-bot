use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::analysis::{AnalysisResult, CreatorProfile};

pub struct AnalysisRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct AnalysisResultDbRow {
    id: Uuid,
    mint_address: String,
    analyzer_name: String,
    score: f64,
    indicators: sqlx::types::Json<Vec<String>>,
    computed_at: DateTime<Utc>,
    /// Total row count for the (unpaginated) query, via `COUNT(*) OVER()`.
    /// Only read on the page-listing path; identical on every row.
    #[sqlx(default)]
    total_count: i64,
}

impl From<AnalysisResultDbRow> for AnalysisResult {
    fn from(r: AnalysisResultDbRow) -> Self {
        Self {
            id: r.id,
            mint_address: r.mint_address,
            analyzer_name: r.analyzer_name,
            score: r.score,
            indicators: r.indicators.0,
            computed_at: r.computed_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct CreatorProfileDbRow {
    id: Uuid,
    wallet_address: String,
    tokens_created: i32,
    total_volume_sol: f64,
    suspiciousness_score: f64,
    wash_trade_score: f64,
    last_analyzed_at: Option<DateTime<Utc>>,
    indicators: sqlx::types::Json<Value>,
    /// Total row count for the (unpaginated) query, via `COUNT(*) OVER()`.
    /// Only read on the page-listing path; identical on every row.
    #[sqlx(default)]
    total_count: i64,
}

impl From<CreatorProfileDbRow> for CreatorProfile {
    fn from(r: CreatorProfileDbRow) -> Self {
        Self {
            id: r.id,
            wallet_address: r.wallet_address,
            tokens_created: r.tokens_created,
            total_volume_sol: r.total_volume_sol,
            suspiciousness_score: r.suspiciousness_score,
            wash_trade_score: r.wash_trade_score,
            last_analyzed_at: r.last_analyzed_at,
            indicators: r.indicators.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl AnalysisRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// All analyzer results for a token.
    pub async fn find_by_mint(&self, mint: &str) -> anyhow::Result<Vec<AnalysisResult>> {
        let rows = sqlx::query_as::<_, AnalysisResultDbRow>(
            "SELECT * FROM tokens_analysis WHERE mint_address = $1",
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(AnalysisResult::from).collect())
    }

    pub async fn find_creator_profile(
        &self,
        wallet: &str,
    ) -> anyhow::Result<Option<CreatorProfile>> {
        let row = sqlx::query_as::<_, CreatorProfileDbRow>(
            "SELECT * FROM creator_profiles WHERE wallet_address = $1",
        )
        .bind(wallet)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(CreatorProfile::from))
    }

    /// Paginated list of all analysis results, newest first.
    /// Returns `(total_count, page_items)`.
    pub async fn list_results(
        &self,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(i64, Vec<AnalysisResult>)> {
        // One round-trip: the windowed `COUNT(*) OVER()` rides on the page query,
        // so the total comes from the first row (0 when the page is empty).
        let rows = sqlx::query_as::<_, AnalysisResultDbRow>(
            "SELECT *, COUNT(*) OVER() AS total_count \
             FROM tokens_analysis ORDER BY computed_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total = rows.first().map(|r| r.total_count).unwrap_or(0);
        Ok((total, rows.into_iter().map(AnalysisResult::from).collect()))
    }

    /// Paginated list of all creator profiles, most suspicious first.
    /// Returns `(total_count, page_items)`.
    pub async fn list_creator_profiles(
        &self,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(i64, Vec<CreatorProfile>)> {
        // One round-trip: the windowed `COUNT(*) OVER()` rides on the page query,
        // so the total comes from the first row (0 when the page is empty).
        let rows = sqlx::query_as::<_, CreatorProfileDbRow>(
            "SELECT *, COUNT(*) OVER() AS total_count \
             FROM creator_profiles ORDER BY suspiciousness_score DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total = rows.first().map(|r| r.total_count).unwrap_or(0);
        Ok((total, rows.into_iter().map(CreatorProfile::from).collect()))
    }
}

impl Clone for AnalysisRepo {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}
