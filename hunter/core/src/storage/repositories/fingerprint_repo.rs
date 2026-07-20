//! `FingerprintRepo` — CRUD over the `fingerprints` table (0004 redesign).
//!
//! A fingerprint is a token-creation shape shared by many rules (see
//! [`crate::models::Fingerprint`]). `find_or_create` is the sweep-promotion
//! entry point: promoting a winning group reuses an identity-identical row
//! instead of minting duplicates.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::constants::tidy_sol_decimal;
use crate::models::Fingerprint;

#[derive(Clone)]
pub struct FingerprintRepo {
    pool: PgPool,
}

// DB row — keeps sqlx derives out of the domain model.
#[derive(sqlx::FromRow)]
struct FingerprintDbRow {
    id: Uuid,
    name: String,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    init_buy_lamports: Option<i64>,
    max_cost_lamports: Option<i64>,
    spendable_lamports_in: Option<i64>,
    first_slot_buy_lamports: Option<i64>,
    first_slot_sell_lamports: Option<i64>,
    bucket_size_amount: f64,
    ix_labels: Option<Vec<String>>,
    metric_config: sqlx::types::Json<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<FingerprintDbRow> for Fingerprint {
    fn from(r: FingerprintDbRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            cu_limit: r.cu_limit,
            cu_price: r.cu_price,
            init_buy_lamports: r.init_buy_lamports,
            max_cost_lamports: r.max_cost_lamports,
            spendable_lamports_in: r.spendable_lamports_in,
            first_slot_buy_lamports: r.first_slot_buy_lamports,
            first_slot_sell_lamports: r.first_slot_sell_lamports,
            bucket_size_amount: tidy_sol_decimal(r.bucket_size_amount),
            ix_labels: r.ix_labels,
            metric_config: r.metric_config.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// Explicit column list (struct order) — not `SELECT *`.
const FINGERPRINT_COLS: &str = "id, name, cu_limit, cu_price, init_buy_lamports, \
    max_cost_lamports, spendable_lamports_in, first_slot_buy_lamports, \
    first_slot_sell_lamports, bucket_size_amount, ix_labels, metric_config, \
    created_at, updated_at";

/// The identity predicate for [`FingerprintRepo::find_or_create`]: every match
/// axis equal, with `NULL` (= "not part of identity") treated as a value
/// (`IS NOT DISTINCT FROM`). `name` is a label, not identity.
const IDENTITY_WHERE: &str = "cu_limit IS NOT DISTINCT FROM $1 \
    AND cu_price IS NOT DISTINCT FROM $2 \
    AND init_buy_lamports IS NOT DISTINCT FROM $3 \
    AND max_cost_lamports IS NOT DISTINCT FROM $4 \
    AND spendable_lamports_in IS NOT DISTINCT FROM $5 \
    AND first_slot_buy_lamports IS NOT DISTINCT FROM $6 \
    AND first_slot_sell_lamports IS NOT DISTINCT FROM $7 \
    AND bucket_size_amount = $8 \
    AND ix_labels IS NOT DISTINCT FROM $9";

impl FingerprintRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, fp: &Fingerprint) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO fingerprints
                (id, name, cu_limit, cu_price, init_buy_lamports, max_cost_lamports,
                 spendable_lamports_in, first_slot_buy_lamports, first_slot_sell_lamports,
                 bucket_size_amount, ix_labels, metric_config, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
        )
        .bind(fp.id)
        .bind(&fp.name)
        .bind(fp.cu_limit)
        .bind(fp.cu_price)
        .bind(fp.init_buy_lamports)
        .bind(fp.max_cost_lamports)
        .bind(fp.spendable_lamports_in)
        .bind(fp.first_slot_buy_lamports)
        .bind(fp.first_slot_sell_lamports)
        .bind(tidy_sol_decimal(fp.bucket_size_amount))
        .bind(fp.ix_labels.as_ref())
        .bind(sqlx::types::Json(&fp.metric_config))
        .bind(fp.created_at)
        .bind(fp.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, fp: &Fingerprint) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE fingerprints SET
                name = $2,
                cu_limit = $3,
                cu_price = $4,
                init_buy_lamports = $5,
                max_cost_lamports = $6,
                spendable_lamports_in = $7,
                first_slot_buy_lamports = $8,
                first_slot_sell_lamports = $9,
                bucket_size_amount = $10,
                ix_labels = $11,
                metric_config = $12,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(fp.id)
        .bind(&fp.name)
        .bind(fp.cu_limit)
        .bind(fp.cu_price)
        .bind(fp.init_buy_lamports)
        .bind(fp.max_cost_lamports)
        .bind(fp.spendable_lamports_in)
        .bind(fp.first_slot_buy_lamports)
        .bind(fp.first_slot_sell_lamports)
        .bind(tidy_sol_decimal(fp.bucket_size_amount))
        .bind(fp.ix_labels.as_ref())
        .bind(sqlx::types::Json(&fp.metric_config))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find(&self, id: Uuid) -> anyhow::Result<Option<Fingerprint>> {
        let row = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Fingerprint::from))
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Fingerprint>> {
        let rows = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Fingerprint::from).collect())
    }

    /// Delete a fingerprint. Fails (FK) while any `strategy_rules` row still
    /// references it — delete or retarget the rules first.
    pub async fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM fingerprints WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Return the existing identity-identical fingerprint, or persist `fp` as a
    /// new row. Sweep promotion goes through here so equal winning groups map
    /// onto ONE fingerprint (`name` is a label and does not affect identity).
    pub async fn find_or_create(&self, fp: &Fingerprint) -> anyhow::Result<Fingerprint> {
        let existing = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints WHERE {IDENTITY_WHERE} LIMIT 1"
        ))
        .bind(fp.cu_limit)
        .bind(fp.cu_price)
        .bind(fp.init_buy_lamports)
        .bind(fp.max_cost_lamports)
        .bind(fp.spendable_lamports_in)
        .bind(fp.first_slot_buy_lamports)
        .bind(fp.first_slot_sell_lamports)
        .bind(tidy_sol_decimal(fp.bucket_size_amount))
        .bind(fp.ix_labels.as_ref())
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = existing {
            return Ok(row.into());
        }
        self.insert(fp).await?;
        Ok(fp.clone())
    }
}
