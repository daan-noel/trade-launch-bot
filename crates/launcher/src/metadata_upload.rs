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

    let content = build_offchain_json(
        &req.name,
        &req.symbol,
        req.description.as_deref(),
        &image_uri,
        req.twitter.as_deref(),
        req.telegram.as_deref(),
        req.website.as_deref(),
    );
    let uri = pin_json(&client, jwt, &content, &req.template_name)
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

/// Edit-form input for an existing metadata template. The image is optional: a
/// blank picker keeps the row's already-pinned image, and only the off-chain
/// JSON is re-pinned so `uri` stays consistent with the (possibly edited)
/// name/symbol/socials — the row's single-source-of-truth invariant.
#[derive(Debug, Clone)]
pub struct UpdateMetadataTemplateRequest {
    pub template_name: String,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub twitter: Option<String>,
    pub telegram: Option<String>,
    pub website: Option<String>,
    /// New image (base64). `None` keeps the existing pinned `image_uri`.
    pub image_base64: Option<String>,
    pub image_filename: Option<String>,
    pub image_content_type: Option<String>,
}

/// Re-pin (image only if replaced, JSON always) and full-replace the row.
/// Returns `None` when `id` doesn't exist. Like `create`, nothing is written
/// unless every required pin succeeds.
pub async fn update_metadata_template(
    pool: &PgPool,
    settings: &LauncherSettings,
    id: uuid::Uuid,
    req: UpdateMetadataTemplateRequest,
) -> Result<Option<MetadataTemplate>> {
    let Some(existing) = MetadataTemplateRepo::get(pool, id).await? else {
        return Ok(None);
    };

    let jwt = settings
        .pinata_jwt
        .as_deref()
        .context("PINATA_JWT not configured — required to pin token metadata")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build reqwest client for Pinata upload")?;

    // Re-pin the image only when a new one was supplied; otherwise keep the
    // row's existing pin.
    let image_uri = match (
        req.image_base64.as_deref(),
        req.image_filename.as_deref(),
        req.image_content_type.as_deref(),
    ) {
        (Some(b64), Some(filename), Some(content_type)) => {
            let image_bytes = STANDARD.decode(b64.as_bytes()).context("decode image_base64")?;
            Some(
                pin_file(&client, jwt, image_bytes, filename, content_type)
                    .await
                    .context("pin image to Pinata")?,
            )
        }
        _ => existing.image_uri.clone(),
    };

    let content = build_offchain_json(
        &req.name,
        &req.symbol,
        req.description.as_deref(),
        image_uri.as_deref().unwrap_or_default(),
        req.twitter.as_deref(),
        req.telegram.as_deref(),
        req.website.as_deref(),
    );
    let uri = pin_json(&client, jwt, &content, &req.template_name)
        .await
        .context("pin metadata JSON to Pinata")?;

    MetadataTemplateRepo::update(
        pool,
        id,
        &NewMetadataTemplate {
            template_name: req.template_name,
            name: req.name,
            symbol: req.symbol,
            description: req.description,
            twitter: req.twitter,
            telegram: req.telegram,
            website: req.website,
            image_uri,
            uri,
        },
    )
    .await
}

/// Build the standard Metaplex/pump.fun off-chain JSON — the single site both
/// create and update use so the two paths can't drift.
fn build_offchain_json(
    name: &str,
    symbol: &str,
    description: Option<&str>,
    image_uri: &str,
    twitter: Option<&str>,
    telegram: Option<&str>,
    website: Option<&str>,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("name".into(), json!(name));
    metadata.insert("symbol".into(), json!(symbol));
    metadata.insert("description".into(), json!(description.unwrap_or_default()));
    metadata.insert("image".into(), json!(image_uri));
    metadata.insert("showName".into(), json!(true));
    if let Some(v) = twitter {
        metadata.insert("twitter".into(), json!(v));
    }
    if let Some(v) = telegram {
        metadata.insert("telegram".into(), json!(v));
    }
    if let Some(v) = website {
        metadata.insert("website".into(), json!(v));
    }
    serde_json::Value::Object(metadata)
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
