use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::state::{app_state::AppState, sol_price};

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
    let latest = match sol_price::fetch_latest_sol_price().await {
        Ok(price) => {
            state.set_sol_price(Some(price));
            Some(price)
        }
        Err(err) => {
            warn!("Failed to refresh SOL price on request: {err}");
            state.latest_sol_price()
        }
    };

    HttpResponse::Ok().json(SolPriceResponse { usd_rate: latest })
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
