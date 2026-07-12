//! Domain D repos: managed wallets, launch templates, launches, bundles.

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    Bundle, Launch, LaunchTemplate, ManageAction, ManagedWallet, NewLaunch, NewLaunchTemplate,
    NewManagedWallet, SellLadder, TokenPosition, UpdateLaunchTemplate, VolumeBot,
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

    /// Fetch many wallets by id in ONE round trip (order unspecified) — the
    /// position reconciler resolving a mint's holder set to on-chain addresses
    /// without N per-wallet `get`s.
    pub async fn get_many(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<Vec<ManagedWallet>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, ManagedWallet>("SELECT * FROM managed_wallets WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(pool)
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

    /// The balance poller's bounded scan: wallets whose balance must be watched
    /// to drive a promotion — both `generated` (manual funding) and `funding` (an
    /// automated treasury send in flight). Backed by the partial index
    /// `idx_managed_wallets_pollable` (migration `0008`).
    pub async fn find_pollable(pool: &PgPool) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "SELECT * FROM managed_wallets \
             WHERE status IN ('generated','funding') ORDER BY created_at",
        )
        .fetch_all(pool)
        .await?)
    }

    /// Record an observed on-chain balance; auto-promotes to `funded` once the
    /// balance clears `min_funded_lamports` (balance-driven detection — never a
    /// manual "mark funded" toggle, avoids bookkeeping drift). Promotes from
    /// **both** `generated` (manual funding) and `funding` (an automated treasury
    /// send that just landed). A no-op promotion for wallets already past those.
    pub async fn record_balance(
        pool: &PgPool,
        id: Uuid,
        balance_lamports: i64,
        min_funded_lamports: i64,
    ) -> anyhow::Result<ManagedWallet> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "UPDATE managed_wallets SET balance_lamports = $2, balance_checked_at = now(), \
             status = CASE WHEN status IN ('generated','funding') AND $2 >= $3 THEN 'funded' ELSE status END \
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(balance_lamports)
        .bind(min_funded_lamports)
        .fetch_one(pool)
        .await?)
    }

    /// Count `funded` (claimable) wallets of `role` — the top-up target check for
    /// the funding orchestrator (`n = target - funded_count`). Uses the
    /// `idx_managed_wallets_role_funded` partial index.
    pub async fn funded_count(pool: &PgPool, role: &str) -> anyhow::Result<i64> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM managed_wallets WHERE role = $1 AND status = 'funded'",
        )
        .bind(role)
        .fetch_one(pool)
        .await?;
        Ok(n)
    }

    /// Atomically claim up to `count` `generated` wallets of `role` for funding,
    /// flipping them to `funding` and stamping `funding_source` (provenance).
    /// `FOR UPDATE SKIP LOCKED` — safe under a concurrent funder / post-restart
    /// run: a wallet claimed here can't be claimed again until it either lands
    /// (`funding` -> `funded`, poller) or reverts (`funding` -> `generated`, on
    /// send failure). Mirrors [`Self::claim_funded`]. May return fewer than
    /// `count` if the `generated` pool is short.
    pub async fn claim_for_funding(
        pool: &PgPool,
        role: &str,
        count: i64,
        funding_source: &str,
    ) -> anyhow::Result<Vec<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "WITH claimed AS ( \
                SELECT id FROM managed_wallets \
                WHERE role = $1 AND status = 'generated' \
                ORDER BY created_at LIMIT $2 \
                FOR UPDATE SKIP LOCKED \
             ) \
             UPDATE managed_wallets SET status = 'funding', funding_source = $3, reserved_at = now() \
             WHERE id IN (SELECT id FROM claimed) \
             RETURNING *",
        )
        .bind(role)
        .bind(count)
        .bind(funding_source)
        .fetch_all(pool)
        .await?)
    }

    /// Atomically claim ONE specific `generated` wallet by id for funding — the
    /// JIT dev-wallet path (`wallet_funding::fund_for_launch`), where the operator
    /// picks the exact dev wallet in the launch console rather than the funder
    /// draining any-N-of-role. Flips `generated -> funding` + stamps
    /// `funding_source`; the single-row conditional `UPDATE` is concurrency-safe
    /// the same way [`Self::claim_specific`] is. Returns `None` if the wallet
    /// wasn't `generated` (already funding/funded, or claimed elsewhere).
    pub async fn claim_specific_for_funding(
        pool: &PgPool,
        id: Uuid,
        funding_source: &str,
    ) -> anyhow::Result<Option<ManagedWallet>> {
        Ok(sqlx::query_as::<_, ManagedWallet>(
            "UPDATE managed_wallets SET status = 'funding', funding_source = $2, reserved_at = now() \
             WHERE id = $1 AND status = 'generated' RETURNING *",
        )
        .bind(id)
        .bind(funding_source)
        .fetch_optional(pool)
        .await?)
    }

    /// Revert `funding` wallets back to `generated` (a treasury send failed, or a
    /// dry-run claim) and clear the tentative `funding_source`. Guarded no-op for
    /// ids not currently `funding` (mirror [`Self::mark_used`] shape). Returns the
    /// number reverted.
    pub async fn revert_funding(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE managed_wallets SET status = 'generated', funding_source = NULL, reserved_at = NULL \
             WHERE id = ANY($1) AND status = 'funding'",
        )
        .bind(ids)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// TTL sweep for the `funding` state (mirror of [`Self::release_expired_reservations`]).
    /// A `funding` claim stamps `reserved_at` (see [`Self::claim_for_funding`]); a
    /// send that never lands — a dropped manual-mode tx whose SOL never left the
    /// treasury — would otherwise strand the wallet in `funding` forever, invisible
    /// to every claim. This reverts any `funding` wallet older than `cutoff` back to
    /// `generated` (clearing the tentative `funding_source`/`reserved_at`), so the
    /// next funding pass can re-claim it. Safe even if the SOL later lands: the
    /// balance poller promotes a `generated` wallet to `funded` off its real
    /// balance just as it does a `funding` one. Returns the number reverted.
    pub async fn revert_stale_funding(
        pool: &PgPool,
        cutoff: DateTime<Utc>,
    ) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE managed_wallets SET status = 'generated', funding_source = NULL, reserved_at = NULL \
             WHERE status = 'funding' AND reserved_at < $1",
        )
        .bind(cutoff)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
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

    /// Release specific `reserved` wallets back to `funded` (reservation cancelled)
    /// — the atomic-launch cleanup path: a launch that fails before/at bundle build,
    /// or a bundle that drops all attempts, spent nothing (a dropped Jito bundle
    /// executes no txs), so its dev + bundler wallets are fully reusable, not `used`.
    /// Clears `reserved_by_launch_id`/`reserved_at`. Guarded no-op for ids not
    /// currently `reserved`. Returns the number released.
    pub async fn release_reservation(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE managed_wallets SET status = 'funded', reserved_by_launch_id = NULL, reserved_at = NULL \
             WHERE id = ANY($1) AND status = 'reserved'",
        )
        .bind(ids)
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
    ///
    /// Also stamps the wallet's true final on-chain balance (`final_balance_lamports`)
    /// — the caller has just observed it (a sweep leaves the wallet at 0; a
    /// below-dust-floor retire leaves only the residual it read). The balance poller
    /// stops watching a wallet once it leaves `generated`/`funding`, so without this
    /// the row would keep displaying a stale funded-era snapshot forever, reading as
    /// phantom holdings even though the SOL is back in the treasury.
    pub async fn retire(
        pool: &PgPool,
        id: Uuid,
        final_balance_lamports: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE managed_wallets SET status = 'retired', \
             balance_lamports = $2, balance_checked_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(final_balance_lamports)
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

    /// Delete by id; `false` when no row matched. `launches.template_id` is
    /// `ON DELETE SET NULL` (migration `0002`), so past launch records survive.
    pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM launch_templates WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
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

    /// Most-recent launch for a mint — the entry point for post-launch management
    /// (resolve a token back to the dev wallet + bundle that seeded it). A mint is
    /// launched once in practice; `ORDER BY created_at DESC LIMIT 1` is defensive.
    pub async fn find_by_mint(pool: &PgPool, mint_address: &str) -> anyhow::Result<Option<Launch>> {
        Ok(sqlx::query_as::<_, Launch>(
            "SELECT * FROM launches WHERE mint_address = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(mint_address)
        .fetch_optional(pool)
        .await?)
    }

    /// Newest-first page of launches enriched for the "launched tokens" list —
    /// each launch LEFT JOINed to its `tokens` identity, the `token_overview`
    /// derived view (trade count / USD price / USD market cap — the SSOT for
    /// display+USD, never recomputed here), and its bundle status. LEFT JOINs so a
    /// launch still lists before its create tx is ingested (name/market are then
    /// `NULL`). Bounded by `limit`/`offset` — never an unbounded scan.
    pub async fn list_page(
        pool: &PgPool,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<crate::models::LaunchListRow>> {
        Ok(sqlx::query_as::<_, crate::models::LaunchListRow>(
            "SELECT l.id, l.template_id, l.mint_address, l.variant, l.status, \
                    l.create_signature, l.dev_buy_quote, l.bundle_id, \
                    b.status AS bundle_status, l.created_at, \
                    t.name, t.symbol, \
                    ov.trade_count, ov.is_migrated, ov.is_dead, \
                    ov.price_usd, ov.market_cap_usd \
             FROM launches l \
             LEFT JOIN tokens t ON t.mint_address = l.mint_address \
             LEFT JOIN token_overview ov ON ov.mint_address = l.mint_address \
             LEFT JOIN bundles b ON b.id = l.bundle_id \
             ORDER BY l.created_at DESC \
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?)
    }

    /// Total launch count — pairs with [`Self::list_page`] for pagination.
    pub async fn count(pool: &PgPool) -> anyhow::Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM launches")
            .fetch_one(pool)
            .await?;
        Ok(n)
    }

    /// Stamp the create leg's tx signature WITHOUT changing status — the atomic
    /// launch bundle sets this at submit time (the create leg is `tx0`), then the
    /// confirm watcher flips status to `created` only once the bundle lands. A
    /// re-bid overwrites it with the new attempt's signature.
    pub async fn set_create_signature(
        pool: &PgPool,
        id: Uuid,
        create_signature: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE launches SET create_signature = $2 WHERE id = $1")
            .bind(id)
            .bind(create_signature)
            .execute(pool)
            .await?;
        Ok(())
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
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        pool: &PgPool,
        launch_id: Uuid,
        tip_quote: Option<i64>,
        legs: serde_json::Value,
        plan: serde_json::Value,
        audit: serde_json::Value,
        create_args: Option<serde_json::Value>,
    ) -> anyhow::Result<Bundle> {
        Ok(sqlx::query_as::<_, Bundle>(
            "INSERT INTO bundles (launch_id, tip_quote, legs, plan, audit, create_args, status) \
             VALUES ($1,$2,$3,$4,$5,$6,'planned') RETURNING *",
        )
        .bind(launch_id)
        .bind(tip_quote)
        .bind(legs)
        .bind(plan)
        .bind(audit)
        .bind(create_args)
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

    /// Atomically claim a `planned` bundle for submission (compare-and-swap):
    /// flips `planned → submitting` in a single conditional UPDATE and reports
    /// whether THIS caller won. Two concurrent `execute` calls both read a bundle
    /// as `planned`, then race to build+submit their own signed tx sets from the
    /// same bundler wallets — a double real-SOL buy. The row-conditional
    /// `WHERE status = 'planned'` lets exactly one caller transition it; the loser
    /// gets `false` (0 rows affected) and must abort before signing/submitting.
    pub async fn claim_for_submitting(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "UPDATE bundles SET status = 'submitting' WHERE id = $1 AND status = 'planned'",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Record the Jito submit result: status → `submitted`, stamps
    /// `submitted_at` (the confirm watcher's timeout window starts here), and
    /// increments `submit_attempts` — so the NEXT re-bid (after a `dropped`
    /// verdict) reads a higher escalation level than the attempt that just lost.
    pub async fn set_submitted(
        pool: &PgPool,
        id: Uuid,
        jito_bundle_id: &str,
        leg_signatures: &[String],
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE bundles SET status = 'submitted', jito_bundle_id = $2, \
             leg_signatures = $3, submitted_at = now(), \
             submit_attempts = submit_attempts + 1 WHERE id = $1",
        )
        .bind(id)
        .bind(jito_bundle_id)
        .bind(leg_signatures)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Stamp the whole-bundle Jito tip actually sized for this submission (lamports)
    /// — the live-floor total the create leg carries. Previously always NULL (the
    /// value only lived inside `legs[].structure`); this is the column the UI/audit
    /// reads. Overwritten each re-bid with the escalated tip.
    pub async fn set_tip_quote(pool: &PgPool, id: Uuid, tip_quote: i64) -> anyhow::Result<()> {
        sqlx::query("UPDATE bundles SET tip_quote = $2 WHERE id = $1")
            .bind(id)
            .bind(tip_quote)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Reset a `submitted`-but-`dropped` bundle back to `planned` for a re-bid,
    /// clearing the stale Jito id / leg signatures / submit timestamp so the
    /// confirm watcher can't re-pick it and the fresh submit stamps clean values.
    /// `submit_attempts` is deliberately preserved (it's the escalation level).
    pub async fn reset_for_rebid(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE bundles SET status = 'planned', jito_bundle_id = NULL, \
             leg_signatures = '{}', submitted_at = NULL WHERE id = $1",
        )
        .bind(id)
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

/// `token_positions` — per-wallet holdings of our launched tokens (post-launch
/// management read model). Seed writes are idempotent; balance writes come from
/// the on-chain reconcile.
pub struct TokenPositionRepo;

impl TokenPositionRepo {
    /// Seed the cost-basis row for a (mint, wallet) the FIRST time it's observed
    /// (dev buy / bundle leg). Idempotent: a re-seed is a no-op (`ON CONFLICT DO
    /// NOTHING`) so it never clobbers a later, more-accurate `cost_quote` or the
    /// reconciled balance. Returns the row (freshly inserted or pre-existing).
    pub async fn seed(
        pool: &PgPool,
        mint_address: &str,
        managed_wallet_id: Uuid,
        role: &str,
        cost_quote: i64,
    ) -> anyhow::Result<TokenPosition> {
        // `wallet_address` is denormalized from managed_wallets by subselect so the
        // caller never has to resolve it and the row can't carry a stale address.
        // Two statements rather than `INSERT ... RETURNING` because a conflicting
        // insert returns no row — always end with a SELECT of the final state.
        sqlx::query(
            "INSERT INTO token_positions (mint_address, managed_wallet_id, wallet_address, role, cost_quote) \
             VALUES ($1, $2, (SELECT address FROM managed_wallets WHERE id = $2), $3, $4) \
             ON CONFLICT (mint_address, managed_wallet_id) DO NOTHING",
        )
        .bind(mint_address)
        .bind(managed_wallet_id)
        .bind(role)
        .bind(cost_quote)
        .execute(pool)
        .await?;
        Ok(sqlx::query_as::<_, TokenPosition>(
            "SELECT * FROM token_positions WHERE mint_address = $1 AND managed_wallet_id = $2",
        )
        .bind(mint_address)
        .bind(managed_wallet_id)
        .fetch_one(pool)
        .await?)
    }

    /// Seed a bundler leg whose bundle NEVER landed: it bought nothing and spent
    /// nothing, so record a zero cost basis and the terminal `dropped` status.
    /// Idempotent (`DO NOTHING`); `set_balance` deliberately never flips a
    /// `dropped` row to open/closed, so this label sticks across reconciles.
    pub async fn seed_dropped(
        pool: &PgPool,
        mint_address: &str,
        managed_wallet_id: Uuid,
        role: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO token_positions (mint_address, managed_wallet_id, wallet_address, role, cost_quote, status) \
             VALUES ($1, $2, (SELECT address FROM managed_wallets WHERE id = $2), $3, 0, 'dropped') \
             ON CONFLICT (mint_address, managed_wallet_id) DO NOTHING",
        )
        .bind(mint_address)
        .bind(managed_wallet_id)
        .bind(role)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// All positions for a mint (the token detail holdings panel), newest-seeded
    /// first. Bounded to one mint's small holder set — never a table scan.
    pub async fn by_mint(pool: &PgPool, mint_address: &str) -> anyhow::Result<Vec<TokenPosition>> {
        Ok(sqlx::query_as::<_, TokenPosition>(
            "SELECT * FROM token_positions WHERE mint_address = $1 ORDER BY role, created_at",
        )
        .bind(mint_address)
        .fetch_all(pool)
        .await?)
    }

    /// The set of managed-wallet ids that still hold an OPEN position — i.e. the
    /// wallet holds tokens (or is a just-launched dev/bundler row seeded before its
    /// first balance reconcile, which defaults `open`). The dust sweep consults
    /// this so it NEVER drains such a wallet's SOL: that SOL is the gas the wallet
    /// needs to sell the position later, and a fee-payer at 0 lamports can't land
    /// the sell (the leader drops it → the manage action reports "confirmation
    /// timed out"). A `closed` (sold out) or `dropped` (never bought) position does
    /// NOT protect a wallet, so a genuinely-terminal wallet still sweeps. One
    /// indexed scan of the small own-holdings table.
    pub async fn managed_wallet_ids_with_open_positions(
        pool: &PgPool,
    ) -> anyhow::Result<std::collections::HashSet<Uuid>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT DISTINCT managed_wallet_id FROM token_positions WHERE status = $1",
        )
        .bind(crate::models::PositionStatus::Open.as_str())
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Write a reconciled on-chain balance + the canonical token account. Auto
    /// `closed` once the balance hits 0 (drained), back to `open` if it's non-zero
    /// (a later buy re-opened it). A `dropped` leg (bundle never landed) is terminal
    /// and left untouched — it never bought, so a 0-balance reconcile must NOT
    /// relabel it `closed` (which reads as "bought then fully sold"). Stamps
    /// `balance_checked_at` + `updated_at`.
    pub async fn set_balance(
        pool: &PgPool,
        id: Uuid,
        balance_base: i64,
        token_account: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE token_positions SET balance_base = $2, \
                 token_account = COALESCE($3, token_account), \
                 status = CASE WHEN status = 'dropped' THEN 'dropped' \
                               WHEN $2 > 0 THEN 'open' ELSE 'closed' END, \
                 balance_checked_at = now(), updated_at = now() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(balance_base)
        .bind(token_account)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Set the feed-derived realized proceeds (sum of the wallet's sell fills for
    /// this mint, quote base units) — authoritative from `trades`, never a
    /// fabricated estimate. Stamps `updated_at`.
    pub async fn set_realized(pool: &PgPool, id: Uuid, realized_quote: i64) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE token_positions SET realized_quote = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(realized_quote)
        .execute(pool)
        .await?;
        Ok(())
    }
}

/// `manage_actions` — audit log of executed post-launch management actions.
pub struct ManageActionRepo;

impl ManageActionRepo {
    /// Record an action about to run (status `executing`). `plan` is the executed
    /// leg set; `selection` the operator's pick. Returns the inserted row.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_executing(
        pool: &PgPool,
        mint_address: &str,
        kind: &str,
        sizing: &str,
        selection: serde_json::Value,
        plan: serde_json::Value,
        plan_orchestrator: serde_json::Value,
        audit: serde_json::Value,
        legs_total: i32,
    ) -> anyhow::Result<ManageAction> {
        Ok(sqlx::query_as::<_, ManageAction>(
            "INSERT INTO manage_actions \
                (mint_address, kind, sizing, selection, plan, plan_orchestrator, audit, status, legs_total) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,'executing',$8) RETURNING *",
        )
        .bind(mint_address)
        .bind(kind)
        .bind(sizing)
        .bind(selection)
        .bind(plan)
        .bind(plan_orchestrator)
        .bind(audit)
        .bind(legs_total)
        .fetch_one(pool)
        .await?)
    }

    /// Terminal outcome: status + confirmed-leg count + optional error + the
    /// executed plan (now carrying per-leg signatures/outcomes). Stamps
    /// `completed_at`.
    pub async fn set_result(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        legs_confirmed: i32,
        plan: serde_json::Value,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE manage_actions SET status = $2, legs_confirmed = $3, plan = $4, \
                 error = $5, completed_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(legs_confirmed)
        .bind(plan)
        .bind(error)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// A mint's action history, newest first (backed by the mint+created_at index).
    pub async fn by_mint(
        pool: &PgPool,
        mint_address: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<ManageAction>> {
        Ok(sqlx::query_as::<_, ManageAction>(
            "SELECT * FROM manage_actions WHERE mint_address = $1 \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(mint_address)
        .bind(limit.max(1))
        .fetch_all(pool)
        .await?)
    }
}

/// `sell_ladders` — armed take-profit ladders + the background evaluator's scan.
pub struct SellLadderRepo;

impl SellLadderRepo {
    /// Arm a new ladder (status `armed`). `rungs` is the `{metric,threshold,pct,
    /// fired}` array; `selection` the wallet pick each fired rung sells across.
    pub async fn insert(
        pool: &PgPool,
        mint_address: &str,
        selection: serde_json::Value,
        rungs: serde_json::Value,
    ) -> anyhow::Result<SellLadder> {
        Ok(sqlx::query_as::<_, SellLadder>(
            "INSERT INTO sell_ladders (mint_address, selection, rungs) \
             VALUES ($1,$2,$3) RETURNING *",
        )
        .bind(mint_address)
        .bind(selection)
        .bind(rungs)
        .fetch_one(pool)
        .await?)
    }

    /// Every ladder for a mint (any status), newest first — the token detail list.
    pub async fn by_mint(pool: &PgPool, mint_address: &str) -> anyhow::Result<Vec<SellLadder>> {
        Ok(sqlx::query_as::<_, SellLadder>(
            "SELECT * FROM sell_ladders WHERE mint_address = $1 ORDER BY created_at DESC",
        )
        .bind(mint_address)
        .fetch_all(pool)
        .await?)
    }

    /// All ARMED ladders across mints — the evaluator's scan. Bounded to the tiny
    /// armed set by the partial index in migration `0012`.
    pub async fn list_armed(pool: &PgPool) -> anyhow::Result<Vec<SellLadder>> {
        Ok(sqlx::query_as::<_, SellLadder>(
            "SELECT * FROM sell_ladders WHERE status = 'armed' ORDER BY created_at",
        )
        .fetch_all(pool)
        .await?)
    }

    /// Persist the evaluator's rung updates + (possibly) a terminal status. Stamps
    /// `updated_at`.
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        rungs: serde_json::Value,
        status: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sell_ladders SET rungs = $2, status = $3, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(rungs)
        .bind(status)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Cancel an armed ladder (operator). Guarded to `armed` so a done ladder
    /// isn't reopened. Returns whether a row changed.
    pub async fn cancel(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE sell_ladders SET status = 'cancelled', updated_at = now() \
             WHERE id = $1 AND status = 'armed'",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }
}

/// `volume_bots` — autonomous volume-making bots + the scheduler's due scan.
pub struct VolumeBotRepo;

impl VolumeBotRepo {
    /// Start a new bot (status `running`, due immediately). `config` is a
    /// `VolumeConfig` JSON; `selection` the wallet pool it rotates across.
    pub async fn insert(
        pool: &PgPool,
        mint_address: &str,
        selection: serde_json::Value,
        config: serde_json::Value,
    ) -> anyhow::Result<VolumeBot> {
        Ok(sqlx::query_as::<_, VolumeBot>(
            "INSERT INTO volume_bots (mint_address, selection, config) \
             VALUES ($1,$2,$3) RETURNING *",
        )
        .bind(mint_address)
        .bind(selection)
        .bind(config)
        .fetch_one(pool)
        .await?)
    }

    /// Every bot for a mint (any status), newest first — the token detail list.
    pub async fn by_mint(pool: &PgPool, mint_address: &str) -> anyhow::Result<Vec<VolumeBot>> {
        Ok(sqlx::query_as::<_, VolumeBot>(
            "SELECT * FROM volume_bots WHERE mint_address = $1 ORDER BY created_at DESC",
        )
        .bind(mint_address)
        .fetch_all(pool)
        .await?)
    }

    /// RUNNING bots whose next cycle is due (`next_run_at <= now()`) — the
    /// scheduler's scan. Bounded to the due set by the partial index in migration
    /// `0013`. Oldest-due first so a backlog drains fairly.
    pub async fn list_due(pool: &PgPool) -> anyhow::Result<Vec<VolumeBot>> {
        Ok(sqlx::query_as::<_, VolumeBot>(
            "SELECT * FROM volume_bots \
             WHERE status = 'running' AND next_run_at <= now() ORDER BY next_run_at",
        )
        .fetch_all(pool)
        .await?)
    }

    /// Record one executed cycle: bump `cycles_done`, add the cycle's buy spend +
    /// traded notional to the running totals, stamp the next due time, and set (or
    /// clear) the last error. Stamps `updated_at`.
    pub async fn record_cycle(
        pool: &PgPool,
        id: Uuid,
        spent_quote_delta: i64,
        volume_quote_delta: i64,
        next_run_at: DateTime<Utc>,
        last_error: Option<&str>,
    ) -> anyhow::Result<VolumeBot> {
        Ok(sqlx::query_as::<_, VolumeBot>(
            "UPDATE volume_bots SET \
                 cycles_done = cycles_done + 1, \
                 spent_quote = spent_quote + $2, \
                 volume_quote = volume_quote + $3, \
                 next_run_at = $4, \
                 last_error = $5, \
                 updated_at = now() \
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(spent_quote_delta)
        .bind(volume_quote_delta)
        .bind(next_run_at)
        .bind(last_error)
        .fetch_one(pool)
        .await?)
    }

    /// Pause a running bot (operator) — the scheduler then skips it. Guarded to
    /// `running`. Returns whether a row changed.
    pub async fn pause(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE volume_bots SET status = 'paused', updated_at = now() \
             WHERE id = $1 AND status = 'running'",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Resume a paused bot (operator), due immediately. Guarded to `paused` so a
    /// stopped bot can't be reopened. Returns whether a row changed.
    pub async fn resume(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE volume_bots SET status = 'running', next_run_at = now(), updated_at = now() \
             WHERE id = $1 AND status = 'paused'",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Terminal stop (operator, budget/max reached). Unlike pause/resume this has
    /// no source-status guard — a bot can be stopped from any live state. Idempotent
    /// (`WHERE status != 'stopped'`). Returns whether a row changed.
    pub async fn stop(pool: &PgPool, id: Uuid, reason: Option<&str>) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE volume_bots SET status = 'stopped', last_error = COALESCE($2, last_error), \
                 updated_at = now() WHERE id = $1 AND status != 'stopped'",
        )
        .bind(id)
        .bind(reason)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }
}
