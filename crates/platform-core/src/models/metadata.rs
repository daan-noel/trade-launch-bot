//! Authored token-metadata templates — content (name/symbol/description/socials)
//! resolved to a pinned image + metadata-JSON `uri` (see
//! `launcher::metadata_upload::create_metadata_template`). The `uri` here is the
//! same shape `launch_templates.params.uri` / `LaunchRequest.uri` consumes.

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
    /// `ipfs://<cid>` — the pinned image.
    pub image_uri: String,
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
    pub image_uri: String,
    pub uri: String,
}
