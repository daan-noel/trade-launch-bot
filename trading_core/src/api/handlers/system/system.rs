use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::config::constants::{
    SLIPPAGE_MAX_BPS, SLIPPAGE_MIN_BPS, WATCHDOG_CHECK_INTERVAL_FLOOR_SECS,
    WATCHDOG_STALL_TIMEOUT_FLOOR_SECS,
};
use crate::state::core_state::CoreState;
use crate::storage::repositories::settings_repo::keys;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

// ---------------------------------------------------------------------------
// --- CORE (settings/price) ---
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SolPriceResponse {
    pub usd_rate: Option<f64>,
}

pub async fn get_sol_price(state: web::Data<Arc<CoreState>>) -> impl Responder {
    // Serve the cached price maintained by the background SOL-price poller
    // (refreshed on the watch channel every 60s) rather than doing a synchronous
    // CoinGecko fetch on every request.
    let usd_rate = state.latest_sol_price();
    HttpResponse::Ok().json(SolPriceResponse { usd_rate })
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
    /// Buy-side slippage in bps; clamped to `[SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS]`.
    /// Present = set the buy default (supersedes legacy `slippage_bps` on the buy path).
    pub buy_slippage_bps: Option<u64>,
    /// Sell-side slippage in bps; clamped to `[SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS]`.
    /// Present = set the sell default. `0` = no floor (always fills).
    pub sell_slippage_bps: Option<u64>,
    /// Persist raw transaction blobs. Present = flip the ingest raw-persist toggle.
    pub persist_raw: Option<bool>,
    /// Master switch for the ingest liveness watchdog.
    pub watchdog_enabled: Option<bool>,
    /// Watchdog stall window (seconds); floored at `WATCHDOG_STALL_TIMEOUT_FLOOR_SECS`.
    pub watchdog_stall_timeout_secs: Option<u64>,
    /// Watchdog check cadence (seconds); floored at `WATCHDOG_CHECK_INTERVAL_FLOOR_SECS`
    /// and capped at the (effective) stall window so a stall can't slip a full cycle.
    pub watchdog_check_interval_secs: Option<u64>,
    /// Hard ceiling (SOL) on total SOL committed across all open real positions.
    /// `None` = no explicit ceiling (balance-floor guard still applies).
    pub max_committed_sol: Option<f64>,
}

pub async fn get_settings(state: web::Data<Arc<CoreState>>) -> impl Responder {
    HttpResponse::Ok().json(state.settings())
}

pub async fn update_settings(
    state: web::Data<Arc<CoreState>>,
    req: web::Json<UpdateSettingsRequest>,
) -> impl Responder {
    let UpdateSettingsRequest {
        track_mayhem,
        track_post_migration,
        timezone,
        price_unit,
        slippage_bps,
        buy_slippage_bps,
        sell_slippage_bps,
        persist_raw,
        watchdog_enabled,
        watchdog_stall_timeout_secs,
        watchdog_check_interval_secs,
        max_committed_sol,
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
    let buy_slippage_clamped =
        buy_slippage_bps.map(|v| v.clamp(SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS));
    let sell_slippage_clamped =
        sell_slippage_bps.map(|v| v.clamp(SLIPPAGE_MIN_BPS, SLIPPAGE_MAX_BPS));

    // Watchdog clamps: floor the stall window (a too-short window restarts the
    // process on a normal lull), then floor the check cadence and cap it at the
    // *effective* window (incoming value or, if absent, the current one) so the
    // poll can never be as coarse as the stall window itself.
    let timeout_clamped =
        watchdog_stall_timeout_secs.map(|v| v.max(WATCHDOG_STALL_TIMEOUT_FLOOR_SECS));
    let effective_timeout =
        timeout_clamped.unwrap_or_else(|| state.settings().watchdog_stall_timeout_secs);
    let check_clamped = watchdog_check_interval_secs
        .map(|v| v.clamp(WATCHDOG_CHECK_INTERVAL_FLOOR_SECS, effective_timeout));

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
    if let Some(v) = buy_slippage_clamped {
        entries.push((keys::BUY_SLIPPAGE_BPS.key, json!(v)));
    }
    if let Some(v) = sell_slippage_clamped {
        entries.push((keys::SELL_SLIPPAGE_BPS.key, json!(v)));
    }
    if let Some(v) = persist_raw {
        entries.push((keys::PERSIST_RAW.key, json!(v)));
    }
    if let Some(v) = watchdog_enabled {
        entries.push((keys::WATCHDOG_ENABLED.key, json!(v)));
    }
    if let Some(v) = timeout_clamped {
        entries.push((keys::WATCHDOG_STALL_TIMEOUT_SECS.key, json!(v)));
    }
    if let Some(v) = check_clamped {
        entries.push((keys::WATCHDOG_CHECK_INTERVAL_SECS.key, json!(v)));
    }
    if let Some(v) = max_committed_sol {
        entries.push((keys::MAX_COMMITTED_SOL.key, json!(v)));
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
        if let Some(v) = buy_slippage_clamped {
            s.buy_slippage_bps = Some(v);
        }
        if let Some(v) = sell_slippage_clamped {
            s.sell_slippage_bps = Some(v);
        }
        if let Some(v) = persist_raw {
            s.persist_raw = v;
        }
        if let Some(v) = watchdog_enabled {
            s.watchdog_enabled = v;
        }
        if let Some(v) = timeout_clamped {
            s.watchdog_stall_timeout_secs = v;
        }
        if let Some(v) = check_clamped {
            s.watchdog_check_interval_secs = v;
        }
        if let Some(v) = max_committed_sol {
            s.max_committed_sol = Some(v);
        }
        updated = Some(s.clone());
    });

    // `send_modify` runs the closure synchronously, so `updated` is always set.
    HttpResponse::Ok().json(updated.expect("modify_settings closure runs synchronously"))
}
