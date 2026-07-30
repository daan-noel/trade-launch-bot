//! `POST /api/system/reload-caches` — admin reseed of DB-backed in-memory state.

use actix_web::{HttpResponse, Responder};

use crate::services::reload_caches::{self, DeployStateData};

pub async fn reload_caches(state: DeployStateData) -> impl Responder {
    let body = reload_caches::reload_all(state.get_ref()).await;
    if body.ok {
        HttpResponse::Ok().json(body)
    } else {
        HttpResponse::ServiceUnavailable().json(body)
    }
}
