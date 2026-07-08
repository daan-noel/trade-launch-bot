//! `metadata_templates` — authored token-metadata content, pinned to IPFS.

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{MetadataTemplate, NewMetadataTemplate};

pub struct MetadataTemplateRepo;

impl MetadataTemplateRepo {
    pub async fn insert(
        pool: &PgPool,
        t: &NewMetadataTemplate,
    ) -> anyhow::Result<MetadataTemplate> {
        Ok(sqlx::query_as::<_, MetadataTemplate>(
            "INSERT INTO metadata_templates \
                (template_name, name, symbol, description, twitter, telegram, website, image_uri, uri) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *",
        )
        .bind(&t.template_name)
        .bind(&t.name)
        .bind(&t.symbol)
        .bind(&t.description)
        .bind(&t.twitter)
        .bind(&t.telegram)
        .bind(&t.website)
        .bind(&t.image_uri)
        .bind(&t.uri)
        .fetch_one(pool)
        .await?)
    }

    pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<MetadataTemplate>> {
        Ok(
            sqlx::query_as::<_, MetadataTemplate>("SELECT * FROM metadata_templates WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?,
        )
    }

    pub async fn all(pool: &PgPool) -> anyhow::Result<Vec<MetadataTemplate>> {
        Ok(sqlx::query_as::<_, MetadataTemplate>(
            "SELECT * FROM metadata_templates ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?)
    }

    /// Full-replace update. Returns `None` when the id doesn't exist. The
    /// re-pinned `image_uri`/`uri` are resolved by the caller
    /// (`launcher::update_metadata_template`) before this write.
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        t: &NewMetadataTemplate,
    ) -> anyhow::Result<Option<MetadataTemplate>> {
        Ok(sqlx::query_as::<_, MetadataTemplate>(
            "UPDATE metadata_templates SET \
                template_name=$2, name=$3, symbol=$4, description=$5, twitter=$6, \
                telegram=$7, website=$8, image_uri=$9, uri=$10 \
             WHERE id=$1 RETURNING *",
        )
        .bind(id)
        .bind(&t.template_name)
        .bind(&t.name)
        .bind(&t.symbol)
        .bind(&t.description)
        .bind(&t.twitter)
        .bind(&t.telegram)
        .bind(&t.website)
        .bind(&t.image_uri)
        .bind(&t.uri)
        .fetch_optional(pool)
        .await?)
    }

    /// Delete by id; `false` when no row matched. Launch templates that
    /// reference this row have their `metadata_template_id` unset by the FK's
    /// `ON DELETE SET NULL` (migration `0007`), so the delete never blocks.
    pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM metadata_templates WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}
