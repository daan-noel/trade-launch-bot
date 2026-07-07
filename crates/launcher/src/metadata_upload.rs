//! Token-metadata authoring: pin an image + the standard off-chain JSON to
//! Pinata (IPFS), then persist the result as a reusable `metadata_templates`
//! row. This row is the single source of truth for token identity: a
//! `launch_templates.metadata_template_id` FK points at it and the launcher
//! resolves name/symbol/uri from it at create time
//! (`service::execute_launch`) — the metadata template is a content preset, not
//! itself part of the create-tx path.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::models::{MetadataTemplate, NewMetadataTemplate};
use platform_core::storage::repositories::MetadataTemplateRepo;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;

use crate::config::LauncherSettings;

const PINATA_PIN_FILE_URL: &str = "https://api.pinata.cloud/pinning/pinFileToIPFS";
const PINATA_PIN_JSON_URL: &str = "https://api.pinata.cloud/pinning/pinJSONToIPFS";

/// Authoring-form input for one metadata template. The image arrives base64
/// (the frontend's file picker read as a data URL) rather than multipart — one
/// less inbound-parsing dependency for a low-volume admin form; the outbound
/// Pinata calls still use `reqwest::multipart`.
#[derive(Debug, Clone)]
pub struct NewMetadataTemplateRequest {
    pub template_name: String,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub twitter: Option<String>,
    pub telegram: Option<String>,
    pub website: Option<String>,
    pub image_base64: String,
    pub image_filename: String,
    pub image_content_type: String,
}

/// Pin the image, build + pin the standard Metaplex/pump.fun off-chain JSON,
/// and persist the result. Nothing is written to `metadata_templates` unless
/// BOTH pins succeed — a partial upload isn't a useful row, so the caller just
/// resubmits the form rather than the server tracking a retry state.
pub async fn create_metadata_template(
    pool: &PgPool,
    settings: &LauncherSettings,
    req: NewMetadataTemplateRequest,
) -> Result<MetadataTemplate> {
    let jwt = settings
        .pinata_jwt
        .as_deref()
        .context("PINATA_JWT not configured — required to pin token metadata")?;

    let image_bytes = STANDARD
        .decode(req.image_base64.as_bytes())
        .context("decode image_base64")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build reqwest client for Pinata upload")?;

    let image_uri = pin_file(
        &client,
        jwt,
        image_bytes,
        &req.image_filename,
        &req.image_content_type,
    )
    .await
    .context("pin image to Pinata")?;

    let mut metadata = serde_json::Map::new();
    metadata.insert("name".into(), json!(req.name));
    metadata.insert("symbol".into(), json!(req.symbol));
    metadata.insert(
        "description".into(),
        json!(req.description.clone().unwrap_or_default()),
    );
    metadata.insert("image".into(), json!(image_uri));
    metadata.insert("showName".into(), json!(true));
    if let Some(v) = &req.twitter {
        metadata.insert("twitter".into(), json!(v));
    }
    if let Some(v) = &req.telegram {
        metadata.insert("telegram".into(), json!(v));
    }
    if let Some(v) = &req.website {
        metadata.insert("website".into(), json!(v));
    }

    let uri = pin_json(
        &client,
        jwt,
        &serde_json::Value::Object(metadata),
        &req.template_name,
    )
    .await
    .context("pin metadata JSON to Pinata")?;

    MetadataTemplateRepo::insert(
        pool,
        &NewMetadataTemplate {
            template_name: req.template_name,
            name: req.name,
            symbol: req.symbol,
            description: req.description,
            twitter: req.twitter,
            telegram: req.telegram,
            website: req.website,
            image_uri: Some(image_uri),
            uri,
        },
    )
    .await
}

async fn pin_file(
    client: &reqwest::Client,
    jwt: &str,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
) -> Result<String> {
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(content_type)
        .context("invalid image content-type")?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client
        .post(PINATA_PIN_FILE_URL)
        .bearer_auth(jwt)
        .multipart(form)
        .send()
        .await
        .context("Pinata pinFileToIPFS HTTP")?
        .error_for_status()
        .context("Pinata pinFileToIPFS HTTP status")?;
    let v: serde_json::Value = resp
        .json()
        .await
        .context("parse Pinata pinFileToIPFS body")?;
    let hash = v
        .get("IpfsHash")
        .and_then(|h| h.as_str())
        .context("Pinata response missing IpfsHash")?;
    Ok(format!("ipfs://{hash}"))
}

async fn pin_json(
    client: &reqwest::Client,
    jwt: &str,
    content: &serde_json::Value,
    template_name: &str,
) -> Result<String> {
    let body = json!({
        "pinataContent": content,
        "pinataMetadata": { "name": template_name },
    });
    let resp = client
        .post(PINATA_PIN_JSON_URL)
        .bearer_auth(jwt)
        .json(&body)
        .send()
        .await
        .context("Pinata pinJSONToIPFS HTTP")?
        .error_for_status()
        .context("Pinata pinJSONToIPFS HTTP status")?;
    let v: serde_json::Value = resp
        .json()
        .await
        .context("parse Pinata pinJSONToIPFS body")?;
    let hash = v
        .get("IpfsHash")
        .and_then(|h| h.as_str())
        .context("Pinata response missing IpfsHash")?;
    Ok(format!("ipfs://{hash}"))
}
