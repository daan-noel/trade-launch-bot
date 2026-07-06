//! Domain D repos: managed wallets, launch templates, launches, bundles.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    Bundle, Launch, LaunchTemplate, ManagedWallet, NewLaunch, NewLaunchTemplate, NewManagedWallet,
};

/// `managed_wallets` — OUR wallets. Stores a key_ref, never a raw key.
pub struct ManagedWalletRepo;

impl ManagedWalletRepo {
    pub async fn insert(pool: &PgPool, w: &NewManagedWallet) -> anyhow::Result<ManagedWallet> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "INSERT INTO managed_wallets (address, label, role, key_ref, derivation_index) \
             VALUES ($1,$2,$3,$4,$5) RETURNING *",
        )
        .bind(&w.address)
        .bind(&w.label)
        .bind(&w.role)
        .bind(&w.key_ref)
        .bind(w.derivation_index)
        .fetch_one(pool)
        .await?)
    }

    pub async fn by_role(pool: &PgPool, role: &str) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "SELECT * FROM managed_wallets WHERE role = $1 AND is_active ORDER BY created_at",
        )
        .bind(role)
        .fetch_all(pool)
        .await?)
    }

    pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>("SELECT * FROM managed_wallets WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }
}

/// `launch_templates` — authored launch specs.
pub struct LaunchTemplateRepo;

impl LaunchTemplateRepo {
    pub async fn insert(pool: &PgPool, t: &NewLaunchTemplate) -> anyhow::Result<LaunchTemplate> {
        Ok(sqlx::query_as::<_, LaunchTemplate>(
            "INSERT INTO launch_templates (template_name, launchpad_id, variant, quote_asset_id, params) \
             VALUES ($1,$2,$3,$4,$5) RETURNING *",
        )
        .bind(&t.template_name)
        .bind(t.launchpad_id)
        .bind(&t.variant)
        .bind(t.quote_asset_id)
        .bind(t.params.clone().unwrap_or_else(|| json!({})))
        .fetch_one(pool)
        .await?)
    }

    pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<LaunchTemplate>> {
        Ok(sqlx::query_as::<_, LaunchTemplate>("SELECT * FROM launch_templates WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }

    pub async fn all(pool: &PgPool) -> anyhow::Result<Vec<LaunchTemplate>> {
        Ok(sqlx::query_as::<_, LaunchTemplate>(
            "SELECT * FROM launch_templates ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?)
    }
}

/// `launches` — executed launch records.
pub struct LaunchRepo;

impl LaunchRepo {
    pub async fn insert(pool: &PgPool, l: &NewLaunch) -> anyhow::Result<Launch> {
        Ok(sqlx::query_as::<_, Launch>(
            "INSERT INTO launches \
                (template_id, mint_address, launchpad_id, variant, quote_asset_id, \
                 dev_wallet_id, dev_buy_quote, status) \
             VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE($8,'pending')) RETURNING *",
        )
        .bind(l.template_id)
        .bind(&l.mint_address)
        .bind(l.launchpad_id)
        .bind(&l.variant)
        .bind(l.quote_asset_id)
        .bind(l.dev_wallet_id)
        .bind(l.dev_buy_quote)
        .bind(&l.status)
        .fetch_one(pool)
        .await?)
    }

    pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Launch>> {
        Ok(sqlx::query_as::<_, Launch>("SELECT * FROM launches WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }

    /// Record the create tx signature + status once the launch lands on-chain.
    pub async fn set_created(
        pool: &PgPool,
        id: Uuid,
        create_signature: &str,
        status: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE launches SET create_signature = $2, status = $3 WHERE id = $1")
            .bind(id)
            .bind(create_signature)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn set_failed(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE launches SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }
}

/// `bundles` — phase-2 Jito bundle seam.
pub struct BundleRepo;

impl BundleRepo {
    pub async fn insert(
        pool: &PgPool,
        launch_id: Uuid,
        tip_quote: Option<i64>,
        legs: serde_json::Value,
    ) -> anyhow::Result<Bundle> {
        Ok(sqlx::query_as::<_, Bundle>(
            "INSERT INTO bundles (launch_id, tip_quote, legs) VALUES ($1,$2,$3) RETURNING *",
        )
        .bind(launch_id)
        .bind(tip_quote)
        .bind(legs)
        .fetch_one(pool)
        .await?)
    }

    pub async fn by_launch(pool: &PgPool, launch_id: Uuid) -> anyhow::Result<Vec<Bundle>> {
        Ok(
            sqlx::query_as::<_, Bundle>("SELECT * FROM bundles WHERE launch_id = $1 ORDER BY created_at")
                .bind(launch_id)
                .fetch_all(pool)
                .await?,
        )
    }
}
