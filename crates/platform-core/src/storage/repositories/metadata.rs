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
}
