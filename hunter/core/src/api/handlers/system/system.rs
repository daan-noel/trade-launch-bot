use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::config::constants::{
    validate_slippage_bps, WATCHDOG_CHECK_INTERVAL_FLOOR_SECS, WATCHDOG_STALL_TIMEOUT_FLOOR_SECS,
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

/// Deserializer for a **three-state** PATCH field: `Option<Option<T>>` where the
/// outer layer is "did the request mention this key" and the inner one is the
/// nullable value. A plain `Option<T>` collapses absent and `null` into the same
/// `None`, which makes a nullable setting impossible to *clear* — the handler
/// can't tell "don't touch it" from "set it back to blank". Slippage needs the
/// distinction because blank is a real state (buy ⇒ default, sell ⇒ no floor).
fn patch_field<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

/// Partial update for the settings document — omitted fields keep their value.
#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub track_mayhem: Option<bool>,
    pub track_post_migration: Option<bool>,
    pub timezone: Option<String>,
    pub price_unit: Option<String>,
    /// Buy-side slippage in bps (100 = 1%), stored **exactly as sent** — the write
    /// path no longer transforms it. Three-state on the wire (see [`patch_field`]):
    /// absent = untouched, `null` = clear back to blank (⇒ the per-side default),
    /// a number = set it. `0` is a 400.
    #[serde(default, deserialize_with = "patch_field")]
    pub buy_slippage_bps: Option<Option<u64>>,
    /// Sell-side slippage in bps, stored **exactly as sent**, same three states.
    /// `0` is a 400 — `null` (the field cleared in the UI) is how you say "no
    /// floor / sell all".
    #[serde(default, deserialize_with = "patch_field")]
    pub sell_slippage_bps: Option<Option<u64>>,
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
    /// Enable gap-replay on LaserStream reconnect. Present = toggle the setting.
    pub gap_replay_on_reconnect: Option<bool>,
    /// Maximum gap-replay window in seconds. Gaps beyond this use a full re-subscribe.
    pub gap_replay_max_window_secs: Option<u64>,
    /// Copycat guard: skip a token whose `(name, symbol)` was already traded on a
    /// different mint inside the window. Flipping this **on** for the first time
    /// also stamps `duplicate_identity_since` (see [`update_settings`]).
    pub skip_duplicate_identity: Option<bool>,
    /// Copycat-guard memory horizon in hours. `0` is a 400 — the enabled flag is
    /// how you turn the guard off, so a zero window would only mean "on but inert".
    pub duplicate_identity_window_hours: Option<u64>,
}

/// Upper bound on the copycat guard's memory, in hours (1 year). Past this the
/// "rolling window" is just "forever" with an unbounded map behind it.
const DUPLICATE_IDENTITY_WINDOW_MAX_HOURS: u64 = 24 * 365;

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
        buy_slippage_bps,
        sell_slippage_bps,
        persist_raw,
        watchdog_enabled,
        watchdog_stall_timeout_secs,
        watchdog_check_interval_secs,
        max_committed_sol,
        gap_replay_on_reconnect,
        gap_replay_max_window_secs,
        skip_duplicate_identity,
        duplicate_identity_window_hours,
    } = req.into_inner();

    if let Some(pu) = &price_unit {
        if pu != "SOL" && pu != "USD" {
            return HttpResponse::BadRequest().json(ErrorBody {
                error: format!("Invalid price_unit '{pu}' (expected SOL or USD)"),
            });
        }
    }

    // A zero window is refused rather than silently defaulted: `skip_duplicate_identity`
    // is the off switch, so `0` here could only produce a guard that reads as ON
    // while blocking nothing — the false-green shape this codebase keeps paying for.
    if let Some(h) = duplicate_identity_window_hours {
        if h == 0 || h > DUPLICATE_IDENTITY_WINDOW_MAX_HOURS {
            return HttpResponse::BadRequest().json(ErrorBody {
                error: format!(
                    "duplicate_identity_window_hours must be 1..={DUPLICATE_IDENTITY_WINDOW_MAX_HOURS} \
                     (use skip_duplicate_identity=false to disable the guard)"
                ),
            });
        }
    }

    // First off→on flip stamps the "start from now" floor. Written once and never
    // reset: a brief toggle-off must not wipe the memory the guard has built, and
    // after one window has elapsed the stamp stops mattering anyway.
    let stamp_since = skip_duplicate_identity.unwrap_or(false)
        && state.settings().duplicate_identity_since.is_none();

    // Slippage is NOT clamped — a typed value is persisted exactly as sent, so
    // nothing sits between the number the operator typed and the number the trader
    // uses (a floor here once turned "accept any fill" into the tightest possible
    // floor on the bot's own exits). Only `0` is refused, because with the value
    // honored literally it would mean "revert on any movement at all"; `null`
    // (blank) is how you say "no floor" / "use the default".
    for (field, v) in [
        ("buy_slippage_bps", buy_slippage_bps),
        ("sell_slippage_bps", sell_slippage_bps),
    ] {
        if let Err(error) = validate_slippage_bps(field, v.flatten()) {
            return HttpResponse::BadRequest().json(ErrorBody { error });
        }
    }

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
    // `json!(None::<u64>)` is JSON `null`, which `pick` decodes back to `None` —
    // so a cleared field round-trips as blank instead of being skipped.
    if let Some(v) = buy_slippage_bps {
        entries.push((keys::BUY_SLIPPAGE_BPS.key, json!(v)));
    }
    if let Some(v) = sell_slippage_bps {
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
    if let Some(v) = gap_replay_on_reconnect {
        entries.push((keys::GAP_REPLAY_ON_RECONNECT.key, json!(v)));
    }
    if let Some(v) = gap_replay_max_window_secs {
        entries.push((keys::GAP_REPLAY_MAX_WINDOW_SECS.key, json!(v)));
    }
    if let Some(v) = skip_duplicate_identity {
        entries.push((keys::SKIP_DUPLICATE_IDENTITY.key, json!(v)));
    }
    if let Some(v) = duplicate_identity_window_hours {
        entries.push((keys::DUPLICATE_IDENTITY_WINDOW_HOURS.key, json!(v)));
    }
    let since_stamp = stamp_since.then(|| Utc::now().to_rfc3339());
    if let Some(v) = &since_stamp {
        entries.push((keys::DUPLICATE_IDENTITY_SINCE.key, json!(v)));
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
        if let Some(v) = buy_slippage_bps {
            s.buy_slippage_bps = v;
        }
        if let Some(v) = sell_slippage_bps {
            s.sell_slippage_bps = v;
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
        if let Some(v) = gap_replay_on_reconnect {
            s.gap_replay_on_reconnect = v;
        }
        if let Some(v) = gap_replay_max_window_secs {
            s.gap_replay_max_window_secs = v;
        }
        if let Some(v) = skip_duplicate_identity {
            s.skip_duplicate_identity = v;
        }
        if let Some(v) = duplicate_identity_window_hours {
            s.duplicate_identity_window_hours = v;
        }
        if let Some(v) = &since_stamp {
            s.duplicate_identity_since = Some(v.clone());
        }
        updated = Some(s.clone());
    });

    // `send_modify` runs the closure synchronously, so `updated` is always set.
    HttpResponse::Ok().json(updated.expect("modify_settings closure runs synchronously"))
}
