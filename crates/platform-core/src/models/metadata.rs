//! Authored token-metadata templates — content (name/symbol/description/socials)
//! resolved to a pinned image + metadata-JSON `uri` (see
//! `launcher::metadata_upload::create_metadata_template`). This is the single
//! source of truth for token identity: a `launch_templates.metadata_template_id`
//! FK references it and the launcher resolves name/symbol/uri from it at launch.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MetadataTemplate {
    pub id: Uuid,
    pub template_name: String,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub twitter: Option<String>,
    pub telegram: Option<String>,
    pub website: Option<String>,
    /// `ipfs://<cid>` — the pinned image. `None` for a preset authored outside the
    /// pin flow (e.g. a template backfilled from a legacy launch template, migration
    /// `0007`), where the image is embedded in the metadata JSON at `uri`.
    pub image_uri: Option<String>,
    /// `ipfs://<cid>` — the pinned metadata JSON (the on-chain-referenced `uri`).
    pub uri: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewMetadataTemplate {
    pub template_name: String,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub twitter: Option<String>,
    pub telegram: Option<String>,
    pub website: Option<String>,
    pub image_uri: Option<String>,
    pub uri: String,
}
