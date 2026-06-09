use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use crate::services::sol_price;
use crate::state::app_state::AppState;
use crate::storage::repositories::settings_repo::SettingsRepo;

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

/// Partial update for the settings document — omitted fields keep their value.
#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub track_mayhem: Option<bool>,
    pub track_post_migration: Option<bool>,
    pub timezone: Option<String>,
    pub price_unit: Option<String>,
}

pub async fn get_settings(state: web::Data<Arc<AppState>>) -> impl Responder {
    HttpResponse::Ok().json(state.settings())
}

pub async fn update_settings(
    state: web::Data<Arc<AppState>>,
    req: web::Json<UpdateSettingsRequest>,
) -> impl Responder {
    let UpdateSettingsRequest {
        track_mayhem,
        track_post_migration,
        timezone,
        price_unit,
    } = req.into_inner();

    if let Some(pu) = &price_unit {
        if pu != "SOL" && pu != "USD" {
            return HttpResponse::BadRequest().json(ErrorBody {
                error: format!("Invalid price_unit '{pu}' (expected SOL or USD)"),
            });
        }
    }

    // Merge the partial onto current settings (read-modify-write): only fields
    // present in the request change, so concurrent updates of different fields
    // (e.g. the settings page and the header) don't clobber each other.
    let mut next = state.settings();
    if let Some(v) = track_mayhem {
        next.track_mayhem = v;
    }
    if let Some(v) = track_post_migration {
        next.track_post_migration = v;
    }
    if let Some(v) = timezone {
        next.timezone = Some(v);
    }
    if let Some(v) = price_unit {
        next.price_unit = Some(v);
    }

    // Persist first; only publish to the live watch if the write succeeds, so a
    // failed save never leaves the runtime diverged from the stored document.
    let repo = SettingsRepo::new(state.db.clone());
    if let Err(e) = repo.set(&next).await {
        return HttpResponse::InternalServerError().json(ErrorBody {
            error: format!("Failed to persist settings: {e}"),
        });
    }

    state.set_settings(next.clone());

    HttpResponse::Ok().json(next)
}
