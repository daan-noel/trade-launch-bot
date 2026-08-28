//! `FingerprintRepo` — CRUD over the `fingerprints` table.
//!
//! A fingerprint is a token-creation shape shared by many rules (see
//! [`crate::models::Fingerprint`]). `find_or_create` is the sweep-promotion entry
//! point: promoting a winning group reuses an identity-identical row instead of
//! minting duplicates. `name` is a label ([`Fingerprint::auto_name`] when
//! blank/stale); identity ignores it. `list`/`find` rewrite stale auto-labels in
//! place.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use hunter_engine::fingerprint::Criteria;
use hunter_engine::metrics::flow_ix::FlowPatterns;

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
    criteria: sqlx::types::Json<Criteria>,
    wildcard: bool,
    metric_config: sqlx::types::Json<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<FingerprintDbRow> for Fingerprint {
    fn from(r: FingerprintDbRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            criteria: r.criteria.0,
            wildcard: r.wildcard,
            metric_config: r.metric_config.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// Explicit column list (struct order) — not `SELECT *`.
const FINGERPRINT_COLS: &str =
    "id, name, criteria, wildcard, metric_config, created_at, updated_at";

/// The identity predicate for [`FingerprintRepo::find_or_create`]: the same
/// criteria, the same wildcard flag and the same `metric_config`. `name` is a label
/// and is not identity.
///
/// One `jsonb` equality replaces the per-axis column chain this used to be --
/// Postgres normalises `jsonb` key order and numeric text, so the comparison is
/// canonical without anyone canonicalising, and an axis added to the registry needs
/// no edit here. The `fingerprints_identity_uniq` index enforces the same key, so a
/// duplicate cannot be created by a racing writer either.
///
/// `metric_config` is NOT match identity -- it selects no token -- but it IS row
/// identity, because [`crate::models::Fingerprint`] carries it into
/// `EngineState`'s per-fingerprint `m_flow_ix` patterns at reload. Leaving it out
/// made this query return an ARBITRARY row (`LIMIT 1`, no ordering) out of the twelve
/// that share `{}`+`wildcard`: promoting a wildcard group could bind the rule to the
/// `8dtx - GMGN Bot` carrier and then overwrite that carrier's patterns with the
/// sweep's, silently reclassifying flow for every rule already bound to it.
const IDENTITY_WHERE: &str =
    "criteria = $1::jsonb AND wildcard = $2 AND metric_config = $3::jsonb";

impl FingerprintRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a new fingerprint. [`validate_row`] runs first as the backstop for
    /// non-HTTP writers (sweep promotion via [`Self::find_or_create`]), so an
    /// unsatisfiable, criterion-less or mis-configured row can't reach the matcher
    /// through a side door.
    pub async fn insert(&self, fp: &Fingerprint) -> anyhow::Result<()> {
        validate_row(fp)?;
        let name = stored_name(fp);
        sqlx::query(
            r#"
            INSERT INTO fingerprints (id, name, criteria, wildcard, metric_config, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(fp.id)
        .bind(&name)
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
        .bind(sqlx::types::Json(&fp.metric_config))
        .bind(fp.created_at)
        .bind(fp.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Overwrite a fingerprint's criteria. Validated exactly like [`Self::insert`]
    /// — an edit can otherwise strip a live fingerprint to zero criteria (silently
    /// killing every rule bound to it) or to an unsatisfiable range.
    pub async fn update(&self, fp: &Fingerprint) -> anyhow::Result<()> {
        validate_row(fp)?;
        let name = stored_name(fp);
        sqlx::query(
            r#"
            UPDATE fingerprints SET
                name = $2,
                criteria = $3,
                wildcard = $4,
                metric_config = $5,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(fp.id)
        .bind(&name)
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
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
        let mut fp = row.map(Fingerprint::from);
        if let Some(fp) = fp.as_mut() {
            self.persist_legacy_relabel(fp).await?;
        }
        Ok(fp)
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Fingerprint>> {
        let rows = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        let mut fps: Vec<Fingerprint> = rows.into_iter().map(Fingerprint::from).collect();
        for fp in &mut fps {
            if let Err(e) = self.persist_legacy_relabel(fp).await {
                tracing::warn!(id = %fp.id, "fingerprint legacy-name relabel failed: {e}");
            }
        }
        Ok(fps)
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
    /// onto ONE fingerprint (`name` is a label and does not affect identity; see
    /// [`IDENTITY_WHERE`] for why `metric_config` does).
    pub async fn find_or_create(&self, fp: &Fingerprint) -> anyhow::Result<Fingerprint> {
        let existing = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints WHERE {IDENTITY_WHERE} LIMIT 1"
        ))
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
        .bind(sqlx::types::Json(&fp.metric_config))
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = existing {
            let mut existing = Fingerprint::from(row);
            self.persist_legacy_relabel(&mut existing).await?;
            return Ok(existing);
        }
        let mut fresh = fp.clone();
        fresh.ensure_auto_name();
        self.insert(&fresh).await?;
        Ok(fresh)
    }

    /// Persist [`Fingerprint::auto_name`] over a stale auto-label. Nicknames stay.
    /// One-shot, self-healing backfill: a name written in an older grammar (a
    /// retired prefix, or a retired chip such as the bucket width) no longer parses
    /// as generated-and-current, so the first list/find rewrites it.
    async fn persist_legacy_relabel(&self, fp: &mut Fingerprint) -> anyhow::Result<()> {
        if !fp.has_stale_auto_name() {
            return Ok(());
        }
        let new_name = fp.auto_name();
        if new_name == fp.name {
            return Ok(());
        }
        sqlx::query("UPDATE fingerprints SET name = $2, updated_at = now() WHERE id = $1")
            .bind(fp.id)
            .bind(&new_name)
            .execute(&self.pool)
            .await?;
        fp.name = new_name;
        Ok(())
    }
}

/// Every gate a stored row must pass, in ONE place.
///
/// Both halves, not just the criteria half. `metric_config` selects no token, so it
/// reads like a label — but it compiles into the fingerprint's live `m_flow_ix`
/// classifier, and an unknown marker name or a malformed pattern list degrades that to
/// "unconfigured", which reads every flow metric as `NaN`: a rule that silently never
/// fires rather than an error. The HTTP edge checks both for a 400; a non-HTTP writer
/// (sweep promotion) reaches the table through here.
fn validate_row(fp: &Fingerprint) -> anyhow::Result<()> {
    fp.validate().map_err(|e| anyhow::anyhow!("invalid fingerprint: {e}"))?;
    FlowPatterns::validate_metric_config(&fp.metric_config)
        .map_err(|e| anyhow::anyhow!("invalid fingerprint metric_config: {e}"))?;
    Ok(())
}

/// Name written on insert/update: auto-name when the submitted label is blank
/// or a retired generator shape; otherwise the caller's nickname.
fn stored_name(fp: &Fingerprint) -> String {
    if fp.has_stale_auto_name() {
        fp.auto_name()
    } else {
        fp.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::IDENTITY_WHERE;

    /// The migration that creates the identity index, read at compile time.
    const RANGES_MIGRATION: &str =
        include_str!("../../../migrations/0009_fingerprint_criteria_ranges.sql");

    /// Columns `IDENTITY_WHERE` compares, in order.
    fn predicate_columns() -> Vec<&'static str> {
        IDENTITY_WHERE
            .split(" AND ")
            .map(|clause| clause.split(" = ").next().unwrap().trim())
            .collect()
    }

    /// Columns `fingerprints_identity_uniq` is built on, in order. The two `jsonb`
    /// columns are indexed as `md5(col::text)` digests -- a btree row is capped at
    /// ~2704 bytes and the `8dtx` carriers' patterns exceed it -- so the wrapper is
    /// unwrapped here: what must agree is WHICH columns are identity, not how the
    /// index stores them.
    fn index_columns() -> Vec<&'static str> {
        let tail = RANGES_MIGRATION
            .split_once("fingerprints_identity_uniq")
            .expect("the migration creates the identity index")
            .1;
        let cols = tail
            .split_once("ON fingerprints (")
            .expect("the index names its table")
            .1
            // Cut at the statement terminator, not the first `)` -- that one closes
            // the `md5(` wrapper, not the column list.
            .split_once(");")
            .expect("the column list closes")
            .0;
        cols.split(',')
            .map(|c| {
                c.trim()
                    .trim_start_matches("md5(")
                    .trim_end_matches(')')
                    .trim_end_matches("::text")
            })
            .collect()
    }

    /// The no-DB guard for the one fact stored twice: what makes two fingerprint rows
    /// the same row. `find_or_create` reads it as SQL and the index enforces it as a
    /// constraint, so the two drifting apart fails in the worst possible place — the
    /// migration aborts on `could not create unique index` against live data, or (the
    /// silent direction) the query returns an ARBITRARY row out of several the index
    /// would have rejected.
    #[test]
    fn the_identity_predicate_and_the_unique_index_key_on_the_same_columns() {
        assert_eq!(
            predicate_columns(),
            index_columns(),
            "IDENTITY_WHERE and fingerprints_identity_uniq must name the same columns"
        );
    }

    /// `metric_config` is the column that was missing, and it is not obvious: it
    /// selects no token, so it reads like a label. It is not — it compiles into that
    /// fingerprint's live `m_flow_ix` patterns, and eleven `8dtx` rows differ only
    /// there. Named explicitly so removing it fails here rather than in production.
    #[test]
    fn identity_includes_metric_config() {
        assert!(
            predicate_columns().contains(&"metric_config"),
            "metric_config is row identity: {IDENTITY_WHERE}"
        );
    }
}
