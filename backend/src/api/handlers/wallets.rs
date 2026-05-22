use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{
    models::wallet::Wallet, state::app_state::AppState,
    storage::repositories::wallet_repo::WalletRepo,
};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FlagRequest {
    /// Human-readable reason for flagging (e.g. "wash_trader", "rug_puller").
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/wallets/:address` — fetch a wallet's DB record.
pub async fn get_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let address = path.into_inner();
    let repo = WalletRepo::new(state.db.clone());

    match repo.find_by_address(&address).await {
        Ok(Some(wallet)) => HttpResponse::Ok().json(wallet),
        Ok(None) => HttpResponse::NotFound().json(json!({
            "error": "wallet not found",
            "address": address
        })),
        Err(e) => {
            tracing::error!("DB error fetching wallet {address}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `POST /api/wallets/:address/flag` — manually flag a wallet.
///
/// Body: `{ "reason": "wash_trader" }`
///
/// Upserts the wallet if it doesn't exist yet (e.g. a wallet seen off-chain).
pub async fn flag_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<FlagRequest>,
) -> impl Responder {
    let address = path.into_inner();
    let repo = WalletRepo::new(state.db.clone());

    // Find existing record or create a stub so we can flag it
    let mut wallet = match repo.find_by_address(&address).await {
        Ok(Some(w)) => w,
        Ok(None) => Wallet::new(address.clone(), chrono::Utc::now()),
        Err(e) => {
            tracing::error!("DB error fetching wallet {address}: {e}");
            return HttpResponse::InternalServerError().json(json!({ "error": "database error" }));
        }
    };

    wallet.flag(body.into_inner().reason);

    match repo.upsert(&wallet).await {
        Ok(_) => HttpResponse::Ok().json(json!({
            "flagged": true,
            "address": address,
            "flag_reason": wallet.flag_reason
        })),
        Err(e) => {
            tracing::error!("DB error flagging wallet {address}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}

/// `DELETE /api/wallets/:address/flag` — remove a manual flag from a wallet.
pub async fn unflag_wallet(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let address = path.into_inner();
    let repo = WalletRepo::new(state.db.clone());

    match repo.find_by_address(&address).await {
        Ok(Some(mut wallet)) => {
            wallet.is_flagged = false;
            wallet.flag_reason = None;
            match repo.upsert(&wallet).await {
                Ok(_) => HttpResponse::Ok().json(json!({ "flagged": false, "address": address })),
                Err(e) => {
                    tracing::error!("DB error unflagging wallet {address}: {e}");
                    HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
                }
            }
        }
        Ok(None) => HttpResponse::NotFound().json(json!({
            "error": "wallet not found",
            "address": address
        })),
        Err(e) => {
            tracing::error!("DB error fetching wallet {address}: {e}");
            HttpResponse::InternalServerError().json(json!({ "error": "database error" }))
        }
    }
}
