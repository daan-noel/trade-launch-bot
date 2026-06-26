//! Deploy-only live-mode toggle handlers (split out of the core `system` module
//! because they take `DeployState`).

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::state::deploy_state::DeployState;
use crate::storage::repositories::settings_repo::keys;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveModeResponse {
    pub live: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLiveModeRequest {
    pub live: bool,
}

pub async fn get_live_mode(state: web::Data<Arc<DeployState>>) -> impl Responder {
    HttpResponse::Ok().json(LiveModeResponse {
        live: state.is_live(),
    })
}

pub async fn set_live_mode(
    state: web::Data<Arc<DeployState>>,
    req: web::Json<UpdateLiveModeRequest>,
) -> impl Responder {
    // Persist the toggle (its own row) first; only flip the runtime live mode if
    // the write succeeds, so a failed save never leaves the WS task running
    // against a state the DB won't restore on the next boot. The in-memory
    // settings snapshot is updated too, keeping `state.settings().live` in sync.
    let repo = state.settings_repo();
    if let Err(e) = repo.set_one(&keys::LIVE, &req.live).await {
        return HttpResponse::InternalServerError().json(ErrorBody {
            error: format!("Failed to persist live mode: {e}"),
        });
    }
    state.modify_settings(|s| s.live = req.live);

    state.set_live(req.live);
    HttpResponse::Ok().json(LiveModeResponse {
        live: state.is_live(),
    })
}
