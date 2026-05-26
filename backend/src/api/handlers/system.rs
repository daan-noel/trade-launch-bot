use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::state::app_state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveModeResponse {
    pub live: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLiveModeRequest {
    pub live: bool,
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
