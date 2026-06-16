use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::config::constants::{SLIPPAGE_MAX_BPS, SLIPPAGE_MIN_BPS};
use crate::state::app_state::AppState;
use crate::storage::repositories::settings_repo::keys;

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
    // Serve the cached price maintained by the background SOL-price poller
    // (refreshed on the watch channel every 60s) rather than doing a synchronous
    // CoinGecko fetch on every request.
    let usd_rate = state.latest_sol_price();
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
    /// Default trade slippage in basis points (100 = 1%); clamped to
    /// `[SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS]`. Present = set the global default.
    pub slippage_bps: Option<u64>,
    /// Persist raw transaction blobs. Present = flip the ingest raw-persist toggle.
    pub persist_raw: Option<bool>,
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
        slippage_bps,
        persist_raw,
    } = req.into_inner();

    if let Some(pu) = &price_unit {
        if pu != "SOL" && pu != "USD" {
            return HttpResponse::BadRequest().json(ErrorBody {
                error: format!("Invalid price_unit '{pu}' (expected SOL or USD)"),
            });
        }
    }

    // Clamp before both the DB row and the in-memory view see the value.
    let slippage_clamped = slippage_bps.map(|v| v.clamp(SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS));

    // Build the per-key upserts for the fields the request actually sent. Each
    // setting is its own row, so this only touches the mentioned keys.
    let mut entries: Vec<(&str, Value)> = Vec::new();
    if let Some(v) = track_mayhem {
        entries.push((keys::TRACK_MAYHEM.key, json!(v)));
    }
    if let Some(v) = track_post_migration {
        entries.push((keys::TRACK_POST_MIGRATION.key, json!(v)));
    }
    if let Some(v) = &timezone {
        entries.push((keys::TIMEZONE.key, json!(v)));
    }
    if let Some(v) = &price_unit {
        entries.push((keys::PRICE_UNIT.key, json!(v)));
    }
    if let Some(v) = slippage_clamped {
        entries.push((keys::SLIPPAGE_BPS.key, json!(v)));
    }
    if let Some(v) = persist_raw {
        entries.push((keys::PERSIST_RAW.key, json!(v)));
    }

    // Persist (one transaction) first; only publish to the watch channel if the
    // write succeeds, so a failed save never leaves the runtime diverged from the
    // stored settings.
    let repo = state.settings_repo();
    if let Err(e) = repo.set_many(&entries).await {
        return HttpResponse::InternalServerError().json(ErrorBody {
            error: format!("Failed to persist settings: {e}"),
        });
    }

    // Apply only the mentioned fields, atomically under the watch lock, so a
    // concurrent update of *different* fields (the settings page and the header)
    // can't clobber each other on the in-memory view as a whole-struct overwrite
    // would. Snapshot the result inside the closure for the response.
    let mut updated = None;
    state.modify_settings(|s| {
        if let Some(v) = track_mayhem {
            s.track_mayhem = v;
        }
        if let Some(v) = track_post_migration {
            s.track_post_migration = v;
        }
        if let Some(v) = timezone {
            s.timezone = Some(v);
        }
        if let Some(v) = price_unit {
            s.price_unit = Some(v);
        }
        if let Some(v) = slippage_clamped {
            s.slippage_bps = Some(v);
        }
        if let Some(v) = persist_raw {
            s.persist_raw = v;
        }
        updated = Some(s.clone());
    });

    // `send_modify` runs the closure synchronously, so `updated` is always set.
    HttpResponse::Ok().json(updated.expect("modify_settings closure runs synchronously"))
}
