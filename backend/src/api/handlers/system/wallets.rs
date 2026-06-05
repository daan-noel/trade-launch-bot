use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::{
        wallet::validate_solana_address,
        wallet_profile::ProfileType,
    },
    state::app_state::AppState,
    storage::repositories::{
        wallet_profile_repo::WalletProfileRepo,
        wallet_profile_tag_repo::WalletProfileTagRepo,
        wallet_repo::WalletRepo,
    },
};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateProfileRequest {
    pub name: String,
    pub profile_type: ProfileType,
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    pub name: String,
    pub profile_type: ProfileType,
}

#[derive(Deserialize)]
pub struct UpdateProfileTagsRequest {
    pub tag_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
pub struct CreateWalletRequest {
    pub address: String,
    pub is_tracked: Option<bool>,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateWalletRequest {
    pub is_tracked: bool,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTagRequest {
    pub name: String,
    pub color: String,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTagRequest {
    pub name: String,
    pub color: String,
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// Profile handlers
// ---------------------------------------------------------------------------

/// `GET /api/profiles` — list all profiles with their wallets.
pub async fn list_profiles(state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.list_with_wallets().await {
        Ok(profiles) => HttpResponse::Ok().json(profiles),
        Err(e) => {
            tracing::error!("list_profiles: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `GET /api/profiles/{id}` — get one profile with its wallets.
pub async fn get_profile(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.find_with_wallets(path.into_inner()).await {
        Ok(Some(p)) => HttpResponse::Ok().json(p),
        Ok(None) => HttpResponse::NotFound().json(json!({ "error": "profile not found" })),
        Err(e) => {
            tracing::error!("get_profile: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `POST /api/profiles` — create a new profile.
pub async fn create_profile(
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreateProfileRequest>,
) -> impl Responder {
    let body = body.into_inner();
    if body.name.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "error": "name is required" }));
    }
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.insert(&body.name, body.profile_type).await {
        Ok(profile) => HttpResponse::Created().json(profile),
        Err(e) => {
            tracing::error!("create_profile: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `PUT /api/profiles/{id}` — update name / type.
pub async fn update_profile(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateProfileRequest>,
) -> impl Responder {
    let id = path.into_inner();
    let body = body.into_inner();
    if body.name.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "error": "name is required" }));
    }
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.update(id, &body.name, body.profile_type).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "updated": true })),
        Err(e) => {
            tracing::error!("update_profile {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `PUT /api/profiles/{id}/tags` — replace the full tag_ids array for a profile.
pub async fn update_profile_tags(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateProfileTagsRequest>,
) -> impl Responder {
    let id = path.into_inner();
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.update_tag_ids(id, &body.tag_ids).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "updated": true })),
        Err(e) => {
            tracing::error!("update_profile_tags {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `DELETE /api/profiles/{id}` — delete profile and all its wallets (cascade).
pub async fn delete_profile(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    let repo = WalletProfileRepo::new(state.db.clone());
    match repo.delete(id).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "deleted": true })),
        Err(e) => {
            tracing::error!("delete_profile {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

// ---------------------------------------------------------------------------
// Wallet handlers
// ---------------------------------------------------------------------------

/// `POST /api/profiles/{id}/wallets` — add a wallet to a profile.
pub async fn create_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    body: web::Json<CreateWalletRequest>,
) -> impl Responder {
    let profile_id = path.into_inner();
    let body = body.into_inner();

    if let Err(e) = validate_solana_address(&body.address) {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }

    let profile_repo = WalletProfileRepo::new(state.db.clone());
    match profile_repo.find_by_id(profile_id).await {
        Ok(None) => return HttpResponse::NotFound().json(json!({ "error": "profile not found" })),
        Err(e) => {
            tracing::error!("create_wallet profile lookup: {e}");
            return HttpResponse::InternalServerError().json(json!({ "error": "database error" }));
        }
        Ok(Some(_)) => {}
    }

    let wallet = crate::models::wallet::Wallet {
        id: Uuid::new_v4(),
        profile_id,
        address: body.address,
        is_tracked: body.is_tracked.unwrap_or(true),
        comment: body.comment,
        created_at: Utc::now(),
        last_seen_at: None,
    };

    let repo = WalletRepo::new(state.db.clone());
    match repo.insert(&wallet).await {
        Ok(_) => HttpResponse::Created().json(&wallet),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                HttpResponse::Conflict()
                    .json(json!({ "error": "address already exists in another profile" }))
            } else {
                tracing::error!("create_wallet: {e}");
                HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
            }
        }
    }
}

/// `GET /api/wallets/{id}` — get a single wallet.
pub async fn get_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let repo = WalletRepo::new(state.db.clone());
    match repo.find_by_id(path.into_inner()).await {
        Ok(Some(w)) => HttpResponse::Ok().json(w),
        Ok(None) => HttpResponse::NotFound().json(json!({ "error": "wallet not found" })),
        Err(e) => {
            tracing::error!("get_wallet: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `PUT /api/wallets/{id}` — update is_tracked and/or comment.
pub async fn update_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateWalletRequest>,
) -> impl Responder {
    let id = path.into_inner();
    let body = body.into_inner();
    let repo = WalletRepo::new(state.db.clone());
    match repo.update(id, body.is_tracked, body.comment.as_deref()).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "updated": true })),
        Err(e) => {
            tracing::error!("update_wallet {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `DELETE /api/wallets/{id}` — remove a wallet.
pub async fn delete_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    let repo = WalletRepo::new(state.db.clone());
    match repo.delete(id).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "deleted": true })),
        Err(e) => {
            tracing::error!("delete_wallet {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

// ---------------------------------------------------------------------------
// Tag handlers
// ---------------------------------------------------------------------------

/// `GET /api/tags` — list all tags.
pub async fn list_tags(state: web::Data<Arc<AppState>>) -> impl Responder {
    let repo = WalletProfileTagRepo::new(state.db.clone());
    match repo.list().await {
        Ok(tags) => HttpResponse::Ok().json(tags),
        Err(e) => {
            tracing::error!("list_tags: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `POST /api/tags` — create a tag.
pub async fn create_tag(
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreateTagRequest>,
) -> impl Responder {
    let body = body.into_inner();
    if body.name.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "error": "name is required" }));
    }
    if body.color.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "error": "color is required" }));
    }
    let repo = WalletProfileTagRepo::new(state.db.clone());
    match repo.insert(&body.name, &body.color, body.comment.as_deref()).await {
        Ok(tag) => HttpResponse::Created().json(tag),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                HttpResponse::Conflict().json(json!({ "error": "tag name already exists" }))
            } else {
                tracing::error!("create_tag: {e}");
                HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
            }
        }
    }
}

/// `PUT /api/tags/{id}` — update a tag.
pub async fn update_tag(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateTagRequest>,
) -> impl Responder {
    let id = path.into_inner();
    let body = body.into_inner();
    if body.name.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "error": "name is required" }));
    }
    let repo = WalletProfileTagRepo::new(state.db.clone());
    match repo.update(id, &body.name, &body.color, body.comment.as_deref()).await {
        Ok(true) => HttpResponse::Ok().json(json!({ "updated": true })),
        Ok(false) => HttpResponse::NotFound().json(json!({ "error": "tag not found" })),
        Err(e) => {
            tracing::error!("update_tag {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `DELETE /api/tags/{id}` — delete a tag.
pub async fn delete_tag(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    let repo = WalletProfileTagRepo::new(state.db.clone());
    match repo.delete(id).await {
        Ok(true) => HttpResponse::Ok().json(json!({ "deleted": true })),
        Ok(false) => HttpResponse::NotFound().json(json!({ "error": "tag not found" })),
        Err(e) => {
            tracing::error!("delete_tag {id}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}
