use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use crate::services::sol_price;
use crate::state::app_state::AppState;
use crate::storage::repositories::settings_repo::{SettingsRepo, TrackingSettings};

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveModeResponse {
    pub live: bool,
}

#[derive(Debug, Serialize)]
pub struct SolPriceResponse {
    pub usd_rate: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLiveModeRequest {
    pub live: bool,
}

pub async fn get_sol_price(state: web::Data<Arc<AppState>>) -> impl Responder {
    let usd_rate = sol_price::refresh(state.get_ref()).await;
    HttpResponse::Ok().json(SolPriceResponse { usd_rate })
}

pub async fn get_live_mode(state: web::Data<Arc<AppState>>) -> impl Responder {
    HttpResponse::Ok().json(LiveModeResponse {
        live: state.is_live(),
    })
}

pub async fn set_live_mode(
    state: web::Data<Arc<AppState>>,
    req: web::Json<UpdateLiveModeRequest>,
) -> impl Responder {
    state.set_live(req.live);
    HttpResponse::Ok().json(LiveModeResponse {
        live: state.is_live(),
    })
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// Partial update for the tracking policy — omitted fields keep their value.
#[derive(Debug, Deserialize)]
pub struct UpdateTrackingSettingsRequest {
    pub track_mayhem: Option<bool>,
    pub track_post_migration: Option<bool>,
}

fn current_tracking(state: &AppState) -> TrackingSettings {
    TrackingSettings {
        track_mayhem: state.track_mayhem(),
        track_post_migration: state.track_post_migration(),
    }
}

pub async fn get_tracking_settings(state: web::Data<Arc<AppState>>) -> impl Responder {
    HttpResponse::Ok().json(current_tracking(state.get_ref()))
}

pub async fn set_tracking_settings(
    state: web::Data<Arc<AppState>>,
    req: web::Json<UpdateTrackingSettingsRequest>,
) -> impl Responder {
    let next = TrackingSettings {
        track_mayhem: req.track_mayhem.unwrap_or_else(|| state.track_mayhem()),
        track_post_migration: req
            .track_post_migration
            .unwrap_or_else(|| state.track_post_migration()),
    };

    // Persist first; only flip the runtime flags if the write succeeds, so a
    // failed save never leaves the live policy diverged from the stored one.
    let repo = SettingsRepo::new(state.db.clone());
    if let Err(e) = repo.set(next).await {
        return HttpResponse::InternalServerError().json(ErrorBody {
            error: format!("Failed to persist settings: {e}"),
        });
    }

    state.set_track_mayhem(next.track_mayhem);
    state.set_track_post_migration(next.track_post_migration);

    HttpResponse::Ok().json(next)
}
