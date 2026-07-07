//! Domain D repos: managed wallets, launch templates, launches, bundles.

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    Bundle, Launch, LaunchTemplate, ManagedWallet, NewLaunch, NewLaunchTemplate, NewManagedWallet,
    UpdateLaunchTemplate,
};

/// `managed_wallets` — OUR wallets. Stores a key_ref, never a raw key. Lifecycle
/// (`status`) queries below back the fresh-wallet pool (docs/wallet-pool-plan.md
/// Phase 1): batch generation lands rows as `generated`; the rest of this repo is
/// the state machine `generated -> funded -> reserved -> used -> retired`.
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

    /// Not-retired wallets for a role — used by the launch console's wallet
    /// pickers. Broader than "claimable" (see [`Self::claim_funded`] for that).
    pub async fn by_role(pool: &PgPool, role: &str) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "SELECT * FROM managed_wallets WHERE role = $1 AND status != 'retired' ORDER BY created_at",
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

    /// Every wallet regardless of lifecycle status (including `retired`),
    /// optionally scoped to `role` — the Phase 2 Wallet Management admin view
    /// (the full pool, not just the not-retired set).
    pub async fn list_all(pool: &PgPool, role: Option<&str>) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(match role {
            Some(r) => sqlx::query_as::<_, ManagedWallet>(
                "SELECT * FROM managed_wallets WHERE role = $1 ORDER BY created_at DESC",
            )
            .bind(r)
            .fetch_all(pool)
            .await?,
            None => sqlx::query_as::<_, ManagedWallet>(
                "SELECT * FROM managed_wallets ORDER BY created_at DESC",
            )
            .fetch_all(pool)
            .await?,
        })
    }

    /// Wallets in a given lifecycle `status`, optionally scoped to `role`. Backs
    /// the balance poller's bounded scan (`status = 'generated'`, partial index)
    /// and the Phase 2 pool admin view.
    pub async fn find_by_status(
        pool: &PgPool,
        status: &str,
        role: Option<&str>,
    ) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(match role {
            Some(r) => sqlx::query_as::<_, ManagedWallet>(
                "SELECT * FROM managed_wallets WHERE status = $1 AND role = $2 ORDER BY created_at",
            )
            .bind(status)
            .bind(r)
            .fetch_all(pool)
            .await?,
            None => sqlx::query_as::<_, ManagedWallet>(
                "SELECT * FROM managed_wallets WHERE status = $1 ORDER BY created_at",
            )
            .bind(status)
            .fetch_all(pool)
            .await?,
        })
    }

    /// Record an observed on-chain balance; auto-promotes `generated` -> `funded`
    /// once the balance clears `min_funded_lamports` (balance-driven detection —
    /// never a manual "mark funded" toggle, avoids bookkeeping drift). A no-op
    /// promotion for wallets already past `generated`.
    pub async fn record_balance(
        pool: &PgPool,
        id: Uuid,
        balance_lamports: i64,
        min_funded_lamports: i64,
    ) -> anyhow::Result<ManagedWallet> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "UPDATE managed_wallets SET balance_lamports = $2, balance_checked_at = now(), \
             status = CASE WHEN status = 'generated' AND $2 >= $3 THEN 'funded' ELSE status END \
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(balance_lamports)
        .bind(min_funded_lamports)
        .fetch_one(pool)
        .await?)
    }

    /// Atomically claim ONE specific `funded` wallet by id for `launch_id` — the
    /// dev-wallet path, where the operator (not the pool) picks which wallet to
    /// use via the launch console dropdown, so there's no "any N of many" to
    /// pick from. A plain conditional `UPDATE` is still concurrency-safe for a
    /// single targeted row: a second claim of the same id blocks on the row
    /// lock, then re-evaluates `status = 'funded'` after the first commits and
    /// correctly finds no match. Returns `None` if the wallet wasn't `funded`
    /// (already claimed by another launch, or never funded).
    pub async fn claim_specific(
        pool: &PgPool,
        id: Uuid,
        launch_id: Uuid,
    ) -> anyhow::Result<Option<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "UPDATE managed_wallets SET status = 'reserved', reserved_by_launch_id = $2, reserved_at = now() \
             WHERE id = $1 AND status = 'funded' RETURNING *",
        )
        .bind(id)
        .bind(launch_id)
        .fetch_optional(pool)
        .await?)
    }

    /// Atomically claim up to `count` `funded` wallets of `role` for `launch_id`
    /// (`FOR UPDATE SKIP LOCKED` — safe under concurrent launches, same principle
    /// as `BundleRepo::find_awaiting_confirmation`'s bounded scan). May return
    /// fewer than `count` if the pool is short.
    pub async fn claim_funded(
        pool: &PgPool,
        role: &str,
        count: i64,
        launch_id: Uuid,
    ) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "WITH claimed AS ( \
                SELECT id FROM managed_wallets \
                WHERE role = $1 AND status = 'funded' \
                ORDER BY random() LIMIT $2 \
                FOR UPDATE SKIP LOCKED \
             ) \
             UPDATE managed_wallets SET status = 'reserved', reserved_by_launch_id = $3, reserved_at = now() \
             WHERE id IN (SELECT id FROM claimed) \
             RETURNING *",
        )
        .bind(role)
        .bind(count)
        .bind(launch_id)
        .fetch_all(pool)
        .await?)
    }

    /// Release `reserved` wallets whose reservation predates `cutoff` back to
    /// `funded` (TTL sweep — an aborted launch shouldn't strand wallets forever).
    /// Returns the number released.
    pub async fn release_expired_reservations(
        pool: &PgPool,
        cutoff: DateTime<Utc>,
    ) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE managed_wallets SET status = 'funded', reserved_by_launch_id = NULL, reserved_at = NULL \
             WHERE status = 'reserved' AND reserved_at < $1",
        )
        .bind(cutoff)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Terminal `reserved` -> `used` transition on launch/bundle completion.
    /// Never re-selectable afterward. A no-op (`WHERE status = 'reserved'` guard)
    /// for ids not currently reserved. Returns the number transitioned.
    pub async fn mark_used(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE managed_wallets SET status = 'used' WHERE id = ANY($1) AND status = 'reserved'",
        )
        .bind(ids)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Terminal transition to `retired` (dust swept, or manually decommissioned)
    /// — the wallet-pool Phase 4 dust sweep, and the final stop for any wallet.
    /// Unlike the other transitions this has no status guard: a wallet can be
    /// retired from any state.
    pub async fn retire(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE managed_wallets SET status = 'retired' WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

/// `launch_templates` — authored launch specs.
pub struct LaunchTemplateRepo;

impl LaunchTemplateRepo {
    pub async fn insert(pool: &PgPool, t: &NewLaunchTemplate) -> anyhow::Result<LaunchTemplate> {
        Ok(sqlx::query_as::<_, LaunchTemplate>(
            "INSERT INTO launch_templates \
                (template_name, launchpad_id, variant, quote_asset_id, metadata_template_id, params) \
             VALUES ($1,$2,$3,$4,$5,$6) RETURNING *",
        )
        .bind(&t.template_name)
        .bind(t.launchpad_id)
        .bind(&t.variant)
        .bind(t.quote_asset_id)
        .bind(t.metadata_template_id)
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

    /// Full-replace update. `updated_at` isn't trigger-maintained anywhere in
    /// this schema, so it's set explicitly here.
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        t: &UpdateLaunchTemplate,
    ) -> anyhow::Result<Option<LaunchTemplate>> {
        Ok(sqlx::query_as::<_, LaunchTemplate>(
            "UPDATE launch_templates SET template_name=$2, launchpad_id=$3, variant=$4, quote_asset_id=$5, \
             metadata_template_id=$6, params=$7, updated_at=now() WHERE id=$1 RETURNING *",
        )
        .bind(id)
        .bind(&t.template_name)
        .bind(t.launchpad_id)
        .bind(&t.variant)
        .bind(t.quote_asset_id)
        .bind(t.metadata_template_id)
        .bind(t.params.clone().unwrap_or_else(|| json!({})))
        .fetch_optional(pool)
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

    pub async fn set_bundle_id(pool: &PgPool, id: Uuid, bundle_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE launches SET bundle_id = $2 WHERE id = $1")
            .bind(id)
            .bind(bundle_id)
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
            "INSERT INTO bundles (launch_id, tip_quote, legs, status) VALUES ($1,$2,$3,'planned') RETURNING *",
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

    pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Bundle>> {
        Ok(sqlx::query_as::<_, Bundle>("SELECT * FROM bundles WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?)
    }

    pub async fn set_status(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE bundles SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Record the Jito submit result: status → `submitted`, stamps
    /// `submitted_at` (the confirm watcher's timeout window starts here).
    pub async fn set_submitted(
        pool: &PgPool,
        id: Uuid,
        jito_bundle_id: &str,
        leg_signatures: &[String],
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE bundles SET status = 'submitted', jito_bundle_id = $2, \
             leg_signatures = $3, submitted_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(jito_bundle_id)
        .bind(leg_signatures)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Terminal confirm-watcher outcome (`landed` | `dropped` | `partial`).
    pub async fn set_confirmed(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE bundles SET status = $2, confirmed_at = now() WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Bundles awaiting landing confirmation — bounded to the tiny in-flight
    /// set via the partial index on `status = 'submitted'`.
    pub async fn find_awaiting_confirmation(pool: &PgPool) -> anyhow::Result<Vec<Bundle>> {
        Ok(sqlx::query_as::<_, Bundle>(
            "SELECT * FROM bundles WHERE status = 'submitted' ORDER BY submitted_at",
        )
        .fetch_all(pool)
        .await?)
    }
}
