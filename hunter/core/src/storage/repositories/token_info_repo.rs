use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::models::token_info::TokenInfo;
use crate::state::token_metrics::TokenMetricsWrite;

/// Repo over the (clean-rebuild) `tokens_info` metrics table.
///
/// The new `tokens_info` shape (token-storage-plan.md) is mint-keyed and drops
/// the old surrogate `id`, the cached `age`/`market_cap` (derived in the
/// `token_overview` view), `created_at` (redundant with `tokens.created_at`),
/// and the per-venue `last_synced_*` columns (moved to `token_sync_state`).
///
/// The runtime [`TokenInfo`] model still carries those fields (consumed by caches
/// / handlers until Phase 2/3 rewires them), so reads synthesize them: `id` is a
/// fresh uuid, `age`/`market_cap` are `None` (derive at the call site / view),
/// `created_at` mirrors `updated_at`, and `last_synced_at` is read from
/// `token_sync_state`. Sync-watermark methods now read/write `token_sync_state`.
#[derive(Clone)]
pub struct TokenInfoRepo {
    pool: PgPool,
}

/// New-schema metrics columns, in the order [`row_to_info`] consumes them.
const INFO_COLS: &str = "mint_address, ath_price, ath_timestamp, volume_sol, trade_count, \
    last_trade_at, current_price, is_dead, is_migrated, lifetime_secs, \
    first_slot_buy_lamports, first_slot_sell_lamports, updated_at";

/// INSERT column list shared by the single-row [`TokenInfoRepo::upsert_metrics`] and
/// the batched [`TokenInfoRepo::upsert_metrics_many`]. Bind order must match this list;
/// `updated_at` is the 13th column (fed `now()` / a bound timestamp).
const METRICS_INSERT_COLS: &str = "mint_address, ath_price, ath_timestamp, volume_sol, \
    trade_count, last_trade_at, current_price, is_dead, is_migrated, lifetime_secs, \
    first_slot_buy_lamports, first_slot_sell_lamports, updated_at";

/// The `ON CONFLICT … DO UPDATE` tail shared by both metric upsert paths, so the
/// per-column merge rules (ath preserve-on-null, last_trade_at monotonic, is_migrated
/// sticky) can never drift between the single-row and batched writes. Leading space:
/// it is concatenated directly after the `VALUES (…)` clause.
const METRICS_UPSERT_CONFLICT: &str = " ON CONFLICT (mint_address) DO UPDATE \
        SET ath_price = COALESCE(EXCLUDED.ath_price, tokens_info.ath_price), \
            ath_timestamp = CASE WHEN EXCLUDED.ath_price IS NOT NULL \
                            THEN EXCLUDED.ath_timestamp ELSE tokens_info.ath_timestamp END, \
            volume_sol = EXCLUDED.volume_sol, \
            trade_count = EXCLUDED.trade_count, \
            last_trade_at = CASE \
                WHEN EXCLUDED.last_trade_at IS NULL THEN tokens_info.last_trade_at \
                WHEN tokens_info.last_trade_at IS NULL THEN EXCLUDED.last_trade_at \
                WHEN EXCLUDED.last_trade_at > tokens_info.last_trade_at THEN EXCLUDED.last_trade_at \
                ELSE tokens_info.last_trade_at \
            END, \
            current_price = EXCLUDED.current_price, \
            is_dead = EXCLUDED.is_dead, \
            is_migrated = tokens_info.is_migrated OR EXCLUDED.is_migrated, \
            lifetime_secs = COALESCE(EXCLUDED.lifetime_secs, tokens_info.lifetime_secs), \
            first_slot_buy_lamports = EXCLUDED.first_slot_buy_lamports, \
            first_slot_sell_lamports = EXCLUDED.first_slot_sell_lamports, \
            updated_at = EXCLUDED.updated_at";

type InfoRow = (
    String,                  // mint_address
    Option<f64>,             // ath_price
    Option<DateTime<Utc>>,   // ath_timestamp
    f64,                     // volume_sol
    i64,                     // trade_count
    Option<DateTime<Utc>>,   // last_trade_at
    Option<f64>,             // current_price
    bool,                    // is_dead
    bool,                    // is_migrated
    Option<i64>,             // lifetime_secs (not on the TokenInfo model; read+dropped)
    Option<i64>,             // first_slot_buy_lamports  (lamports; → human SOL on read)
    Option<i64>,             // first_slot_sell_lamports (lamports; → human SOL on read)
    DateTime<Utc>,           // updated_at
);

/// Build the runtime [`TokenInfo`] from a new-schema row, synthesizing the fields
/// the new table no longer stores. `last_synced_at` is filled separately (from
/// `token_sync_state`) — `None` here.
fn row_to_info(r: InfoRow) -> TokenInfo {
    let (
        mint_address,
        ath_price,
        ath_timestamp,
        volume_sol,
        trade_count,
        last_trade_at,
        current_price,
        is_dead,
        is_migrated,
        _lifetime_secs,
        first_slot_buy_lamports,
        first_slot_sell_lamports,
        updated_at,
    ) = r;
    TokenInfo {
        id: Uuid::new_v4(),
        mint_address,
        ath_price,
        ath_timestamp,
        age: None,
        volume_sol,
        market_cap: None,
        trade_count,
        last_trade_at,
        current_price,
        is_dead,
        is_migrated,
        // Lamports (BIGINT) → human SOL f64 on read (mirrors `initial_buy_sol`).
        first_slot_buy_sol: first_slot_buy_lamports.map(lamports_to_sol),
        first_slot_sell_sol: first_slot_sell_lamports.map(lamports_to_sol),
        created_at: updated_at,
        updated_at,
        last_synced_at: None,
    }
}

// SOL ↔ lamports use the shared `config::constants` DB-boundary helpers.
// `first_slot_*_sol` are stored as exact lamports (BIGINT) but carried as human SOL
// on the metrics/model side, mirroring `tokens.initial_buy_sol` / `trades.amount_lamports`.

impl TokenInfoRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Durable migration flag for one mint — the sticky `tokens_info.is_migrated`
    /// (set `true` by the ingest migration write, never cleared). The exit loop
    /// consults this when the volatile in-RAM token cache has aged out on a long
    /// hold, so a cache miss can't route a doomed **curve** sell into a migrated
    /// AMM pool. One indexed PK read — off the snipe hot path (exit only). Missing
    /// row ⇒ not migrated.
    pub async fn is_migrated(&self, mint: &str) -> anyhow::Result<bool> {
        let row: Option<(bool,)> =
            sqlx::query_as("SELECT is_migrated FROM tokens_info WHERE mint_address = $1")
                .bind(mint)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(m,)| m).unwrap_or(false))
    }

    /// Upsert token metrics. `age` / `market_cap` are accepted for signature
    /// stability but no longer stored (derived in `token_overview`).
    ///
    /// `ath_price` is written authoritatively from the caller's freshly recomputed
    /// value (a re-sync must be able to *lower* a previously over-stated ATH); a
    /// NULL incoming value preserves the existing stored value.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_metrics(
        &self,
        mint: &str,
        ath_price: Option<f64>,
        ath_timestamp: Option<DateTime<Utc>>,
        _age: Option<i64>,
        volume_sol: f64,
        _market_cap: Option<f64>,
        trade_count: i64,
        last_trade_at: Option<DateTime<Utc>>,
        current_price: Option<f64>,
        is_dead: bool,
        is_migrated: bool,
        lifetime_secs: Option<i64>,
        first_slot_buy_sol: f64,
        first_slot_sell_sol: f64,
    ) -> anyhow::Result<()> {
        // `first_slot_*` do a plain overwrite (not COALESCE-preserve like ath): the
        // value grows monotonically within the open creation-slot window and freezes
        // once closed, so the latest in-memory value is always authoritative. That
        // (and every other per-column merge rule) lives in `METRICS_UPSERT_CONFLICT`,
        // shared with the batched path so the two can't drift.
        sqlx::query(&format!(
            "INSERT INTO tokens_info ({METRICS_INSERT_COLS}) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now()){METRICS_UPSERT_CONFLICT}"
        ))
        .bind(mint)
        .bind(ath_price)
        .bind(ath_timestamp)
        .bind(volume_sol)
        .bind(trade_count)
        .bind(last_trade_at)
        .bind(current_price)
        .bind(is_dead)
        .bind(is_migrated)
        .bind(lifetime_secs)
        // Human SOL → lamports (BIGINT) on write.
        .bind(sol_to_lamports(first_slot_buy_sol))
        .bind(sol_to_lamports(first_slot_sell_sol))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batched form of [`Self::upsert_metrics`] — one multi-row
    /// `INSERT … ON CONFLICT DO UPDATE` per flush instead of one statement per mint.
    /// The db_writer flushes every distinct mint touched in a ~150 ms window; the old
    /// per-mint fan-out held several pool connections at once and issued one
    /// round-trip per mint, making it the ingest write most likely to exhaust the
    /// pool under load. This holds ONE connection for one round-trip per chunk.
    ///
    /// Reuses [`METRICS_INSERT_COLS`] + [`METRICS_UPSERT_CONFLICT`], so the per-column
    /// merge rules are byte-identical to the single-row path (SSOT). `updated_at` is a
    /// single timestamp bound once for the whole batch.
    pub async fn upsert_metrics_many(&self, rows: &[TokenMetricsWrite]) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        // 13 binds/row; a single statement caps at 65535 bind params (the wire
        // protocol's int16 count) and sqlx 0.6 does not guard it, so chunk well under.
        const METRICS_UPSERT_CHUNK: usize = 2000;
        let now = Utc::now();
        for chunk in rows.chunks(METRICS_UPSERT_CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> =
                sqlx::QueryBuilder::new(format!("INSERT INTO tokens_info ({METRICS_INSERT_COLS}) "));
            qb.push_values(chunk, |mut b, m| {
                b.push_bind(&m.mint)
                    .push_bind(m.ath_price)
                    .push_bind(m.ath_timestamp)
                    .push_bind(m.volume_sol)
                    .push_bind(m.trade_count)
                    .push_bind(m.last_trade_at)
                    .push_bind(m.current_price)
                    .push_bind(m.is_dead)
                    .push_bind(m.is_migrated)
                    .push_bind(m.lifetime_secs)
                    // Human SOL → lamports (BIGINT) on write.
                    .push_bind(sol_to_lamports(m.first_slot_buy_sol))
                    .push_bind(sol_to_lamports(m.first_slot_sell_sol))
                    .push_bind(now);
            });
            qb.push(METRICS_UPSERT_CONFLICT);
            qb.build().execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn update_migration_status(
        &self,
        mint: &str,
        is_migrated: bool,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tokens_info (mint_address, is_migrated, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (mint_address) DO UPDATE
                SET is_migrated = tokens_info.is_migrated OR EXCLUDED.is_migrated,
                    updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(mint)
        .bind(is_migrated)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn find_by_mint(&self, mint: &str) -> anyhow::Result<Option<TokenInfo>> {
        let row = sqlx::query_as::<_, InfoRow>(&format!(
            "SELECT {INFO_COLS} FROM tokens_info WHERE mint_address = $1"
        ))
        .bind(mint)
        .fetch_optional(&self.pool)
        .await?;

        let mut info = row.map(row_to_info);
        if let Some(info) = info.as_mut() {
            info.last_synced_at = self.max_synced_at(mint).await?;
        }
        Ok(info)
    }

    /// Token metrics rows for the given mints, batched in chunks of `INFO_CHUNK`.
    /// Scoped to the seeded set (`mint = ANY($1)`) so cold start never `SELECT`s the
    /// whole, retention-free `tokens_info` table. Mints with no row are absent.
    /// `last_synced_at` is left `None` here (the batch seed path does not need it;
    /// fetch per-mint via [`Self::find_by_mint`] when required).
    pub async fn find_for(&self, mints: &[String]) -> anyhow::Result<Vec<TokenInfo>> {
        /// Mints per round-trip; keeps each `= ANY($1)` array index-friendly.
        const INFO_CHUNK: usize = 1000;
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(mints.len());
        for chunk in mints.chunks(INFO_CHUNK) {
            let batch = sqlx::query_as::<_, InfoRow>(&format!(
                "SELECT {INFO_COLS} FROM tokens_info WHERE mint_address = ANY($1)"
            ))
            .bind(chunk)
            .fetch_all(&self.pool)
            .await?;
            out.extend(batch.into_iter().map(row_to_info));
        }
        Ok(out)
    }

    /// Newest successful-sync wall-clock for a mint (MAX over its venue rows).
    async fn max_synced_at(&self, mint: &str) -> anyhow::Result<Option<DateTime<Utc>>> {
        let at: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT MAX(last_synced_at) FROM token_sync_state WHERE mint_address = $1",
        )
        .bind(mint)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(at)
    }

    /// Read the per-token sync watermark across venues:
    /// `(last_synced_at, curve_sig, amm_sig, curve_slot, amm_slot)`.
    /// Now sourced from `token_sync_state` (one row per `(mint, venue)`); any field
    /// is `None` if that venue has never synced. `last_synced_at` is the MAX over
    /// the mint's venue rows.
    pub async fn get_sync_watermark(
        &self,
        mint: &str,
    ) -> anyhow::Result<(
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    )> {
        let rows = sqlx::query_as::<_, (String, Option<String>, Option<i64>, Option<DateTime<Utc>>)>(
            "SELECT venue, last_sig, last_slot, last_synced_at \
             FROM token_sync_state WHERE mint_address = $1",
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        let mut last_synced_at: Option<DateTime<Utc>> = None;
        let (mut curve_sig, mut amm_sig, mut curve_slot, mut amm_slot) = (None, None, None, None);
        for (venue, sig, slot, at) in rows {
            last_synced_at = match (last_synced_at, at) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            match venue.as_str() {
                "curve" => {
                    curve_sig = sig;
                    curve_slot = slot;
                }
                "amm" => {
                    amm_sig = sig;
                    amm_slot = slot;
                }
                _ => {}
            }
        }
        Ok((last_synced_at, curve_sig, amm_sig, curve_slot, amm_slot))
    }

    /// Record a successful sync: upsert the per-venue `token_sync_state` rows with
    /// `last_synced_at = at` and the newest signature/slot seen. A `None`
    /// signature for a venue leaves that venue's row untouched (an AMM-less sync
    /// preserves any existing AMM watermark).
    pub async fn update_sync_watermark(
        &self,
        mint: &str,
        at: DateTime<Utc>,
        curve_sig: Option<&str>,
        amm_sig: Option<&str>,
        curve_slot: Option<i64>,
        amm_slot: Option<i64>,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        for (venue, sig, slot) in [("curve", curve_sig, curve_slot), ("amm", amm_sig, amm_slot)] {
            if sig.is_none() && slot.is_none() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO token_sync_state (mint_address, venue, last_sig, last_slot, last_synced_at)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (mint_address, venue) DO UPDATE
                    SET last_sig = COALESCE(EXCLUDED.last_sig, token_sync_state.last_sig),
                        last_slot = COALESCE(EXCLUDED.last_slot, token_sync_state.last_slot),
                        last_synced_at = EXCLUDED.last_synced_at
                "#,
            )
            .bind(mint)
            .bind(venue)
            .bind(sig)
            .bind(slot)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
