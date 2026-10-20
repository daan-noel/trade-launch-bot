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
    alt_criteria: sqlx::types::Json<Vec<Criteria>>,
    exclude_criteria: sqlx::types::Json<Vec<Criteria>>,
    wildcard: bool,
    tags: sqlx::types::Json<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<FingerprintDbRow> for Fingerprint {
    fn from(r: FingerprintDbRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            criteria: r.criteria.0,
            alt_criteria: r.alt_criteria.0,
            exclude_criteria: r.exclude_criteria.0,
            wildcard: r.wildcard,
            tags: r.tags.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// Explicit column list (struct order) — not `SELECT *`.
const FINGERPRINT_COLS: &str =
    "id, name, criteria, alt_criteria, exclude_criteria, wildcard, tags, created_at, updated_at";

/// The identity predicate for [`FingerprintRepo::find_or_create`]: the same
/// shapes (`criteria`, `alt_criteria`, `exclude_criteria`), the same wildcard flag
/// and the same `tags`. `name` is a label and is not identity. The shape lists are
/// stored canonical (`Fingerprint::canonicalize`), so `jsonb` array equality is
/// set equality.
///
/// One `jsonb` equality replaces the per-axis column chain this used to be --
/// Postgres normalises `jsonb` key order and numeric text, so the comparison is
/// canonical without anyone canonicalising, and an axis added to the registry needs
/// no edit here. The `fingerprints_identity_uniq` index enforces the same key, so a
/// duplicate cannot be created by a racing writer either.
///
/// `tags` is NOT match identity -- it selects no token -- but it IS row
/// identity, because [`crate::models::Fingerprint`] carries it into
/// `EngineState`'s per-fingerprint `m_flow_ix` patterns at reload. Leaving it out
/// made this query return an ARBITRARY row (`LIMIT 1`, no ordering) out of the twelve
/// that share `{}`+`wildcard`: promoting a wildcard group could bind the rule to the
/// `8dtx - GMGN Bot` carrier and then overwrite that carrier's patterns with the
/// sweep's, silently reclassifying flow for every rule already bound to it.
const IDENTITY_WHERE: &str =
    "criteria = $1::jsonb AND alt_criteria = $4::jsonb AND exclude_criteria = $5::jsonb AND wildcard = $2 AND tags = $3::jsonb";

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
            INSERT INTO fingerprints
                (id, name, criteria, wildcard, tags, created_at, updated_at, alt_criteria, exclude_criteria)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(fp.id)
        .bind(&name)
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
        .bind(sqlx::types::Json(&fp.tags))
        .bind(fp.created_at)
        .bind(fp.updated_at)
        .bind(sqlx::types::Json(&fp.alt_criteria))
        .bind(sqlx::types::Json(&fp.exclude_criteria))
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
                tags = $5,
                alt_criteria = $6,
                exclude_criteria = $7,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(fp.id)
        .bind(&name)
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
        .bind(sqlx::types::Json(&fp.tags))
        .bind(sqlx::types::Json(&fp.alt_criteria))
        .bind(sqlx::types::Json(&fp.exclude_criteria))
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

    /// The row that already holds `fp`'s identity, if any — a **read**, so a
    /// caller that must not write (the bundle diff preview) resolves the same
    /// collision the writers do without touching the table.
    ///
    /// Split out of [`Self::find_or_create`] rather than duplicated: a preview
    /// that answered "no collision" off a second, drifted predicate would then
    /// hand the operator an apply that trips `fingerprints_identity_uniq`.
    pub async fn find_by_identity(&self, fp: &Fingerprint) -> anyhow::Result<Option<Fingerprint>> {
        let existing = sqlx::query_as::<_, FingerprintDbRow>(&format!(
            "SELECT {FINGERPRINT_COLS} FROM fingerprints WHERE {IDENTITY_WHERE} LIMIT 1"
        ))
        .bind(sqlx::types::Json(&fp.criteria))
        .bind(fp.wildcard)
        .bind(sqlx::types::Json(&fp.tags))
        .bind(sqlx::types::Json(&fp.alt_criteria))
        .bind(sqlx::types::Json(&fp.exclude_criteria))
        .fetch_optional(&self.pool)
        .await?;
        Ok(existing.map(Fingerprint::from))
    }

    /// Return the existing identity-identical fingerprint, or persist `fp` as a
    /// new row. Sweep promotion goes through here so equal winning groups map
    /// onto ONE fingerprint (`name` is a label and does not affect identity; see
    /// [`IDENTITY_WHERE`] for why `tags` does).
    pub async fn find_or_create(&self, fp: &Fingerprint) -> anyhow::Result<Fingerprint> {
        if let Some(mut existing) = self.find_by_identity(fp).await? {
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
/// Both halves, not just the criteria half. `tags` selects no token, so it
/// reads like a label — but it compiles into the fingerprint's live `m_flow_ix` and
/// `m_dump_ix` classifiers, and an unknown marker name or a malformed pattern list
/// degrades one to "unconfigured", which reads its metrics as `NaN`: a rule that
/// silently never fires rather than an error. The HTTP edge checks both for a 400; a
/// non-HTTP writer (sweep promotion) reaches the table through here.
fn validate_row(fp: &Fingerprint) -> anyhow::Result<()> {
    fp.validate().map_err(|e| anyhow::anyhow!("invalid fingerprint: {e}"))?;
    hunter_engine::metrics::tags::config::validate_tags(&fp.tags)
        .map_err(|e| anyhow::anyhow!("invalid fingerprint tags: {e}"))?;
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

    /// The migration that last (re)creates the identity index, read at compile time.
    const INDEX_MIGRATION: &str =
        include_str!("../../../migrations/0033_fingerprint_shape_lists.sql");

    /// Columns `IDENTITY_WHERE` compares, sorted: the index is a set of columns and
    /// the query names them in bind order.
    fn predicate_columns() -> Vec<&'static str> {
        let mut cols: Vec<&str> = IDENTITY_WHERE
            .split(" AND ")
            .map(|clause| clause.split(" = ").next().unwrap().trim())
            .collect();
        cols.sort_unstable();
        cols
    }

    /// Columns `fingerprints_identity_uniq` is built on, in order. The two `jsonb`
    /// columns are indexed as `md5(col::text)` digests -- a btree row is capped at
    /// ~2704 bytes and the `8dtx` carriers' patterns exceed it -- so the wrapper is
    /// unwrapped here: what must agree is WHICH columns are identity, not how the
    /// index stores them.
    fn index_columns() -> Vec<&'static str> {
        let tail = INDEX_MIGRATION
            .split_once("CREATE UNIQUE INDEX IF NOT EXISTS fingerprints_identity_uniq")
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
        let mut cols: Vec<&str> = cols
            .split(',')
            .map(|c| {
                c.trim()
                    .trim_start_matches("md5(")
                    .trim_end_matches(')')
                    .trim_end_matches("::text")
            })
            .collect();
        cols.sort_unstable();
        cols
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

    /// `tags` is the column that is easy to miss: it selects no token, so it reads
    /// like a label. It is not — it compiles into that fingerprint's live tags, and
    /// eleven `8dtx` rows differ only there. Named explicitly so removing it fails here
    /// rather than in production.
    #[test]
    fn identity_includes_tags() {
        assert!(
            predicate_columns().contains(&"tags"),
            "tags is row identity: {IDENTITY_WHERE}"
        );
    }

    /// The shape lists are identity too: two rows with the same first shape and
    /// different alternatives or exclusions select different tokens.
    #[test]
    fn identity_includes_both_shape_lists() {
        let cols = predicate_columns();
        assert!(cols.contains(&"alt_criteria") && cols.contains(&"exclude_criteria"), "{IDENTITY_WHERE}");
    }

    /// Insert, find by identity, update and find-or-create a multi-shape row against a
    /// real database, then delete it. Leaves nothing behind.
    #[tokio::test]
    #[ignore = "needs DATABASE_URL with core migration 0033 applied; creates and deletes one fingerprint row"]
    async fn a_multi_shape_row_round_trips_through_the_repo() {
        use super::FingerprintRepo;
        use crate::models::Fingerprint;
        use hunter_engine::fingerprint::{AxisId, AxisPredicate, Criteria};

        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&url).await.expect("connect");
        let repo = FingerprintRepo::new(pool);

        let shape = |n: u128| Criteria::new().with(AxisId::IxCount, AxisPredicate::exact(n));
        let mut fp = Fingerprint::empty(uuid::Uuid::new_v4(), chrono::Utc::now());
        fp.criteria = shape(5);
        fp.alt_criteria = vec![shape(4)];
        fp.exclude_criteria = vec![shape(7)];
        // A unique tag document, so this never collides with a real row's identity.
        fp.tags = serde_json::json!({ "repo_test": { "match": { "creator": true } } });
        fp.canonicalize();

        repo.insert(&fp).await.expect("insert");
        let got = async {
            let found = repo.find_by_identity(&fp).await.expect("find_by_identity").expect("found by identity");
            assert_eq!(found.id, fp.id);
            assert_eq!((&found.criteria, &found.alt_criteria, &found.exclude_criteria), (&fp.criteria, &fp.alt_criteria, &fp.exclude_criteria));

            // A different exclusion is a different row: identity does not find it.
            let mut other = fp.clone();
            other.exclude_criteria = vec![shape(6)];
            assert!(repo.find_by_identity(&other).await.expect("find").is_none());

            // find_or_create on the same identity returns the existing row.
            let again = repo.find_or_create(&fp).await.expect("find_or_create");
            assert_eq!(again.id, fp.id);

            // Update the exclusion list, read it back.
            let mut edited = found.clone();
            edited.exclude_criteria = vec![shape(6)];
            repo.update(&edited).await.expect("update");
            let reread = repo.find(fp.id).await.expect("find").expect("row");
            assert_eq!(reread.exclude_criteria, vec![shape(6)]);
            assert_eq!(reread.alt_criteria, fp.alt_criteria, "the other list is untouched");
        }
        .await;
        repo.delete(fp.id).await.expect("cleanup");
        got
    }
}
