use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::state::deploy_state::DeployState;

/// GET /api/wallet/rent
///
/// One scan of both token programs, plus the open-position rows. Not polled:
/// the wallet card reads it on mount and after a recover.
pub async fn get_rent_status(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match crate::services::rent::status(app_state.get_ref()).await {
        Ok(status) => HttpResponse::Ok().json(status),
        Err(e) => {
            tracing::warn!("get_rent_status failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct RecoverRequest {
    /// Also burn dust balances and close those accounts. Absent closes empty
    /// accounts and unwraps wrapped SOL only.
    #[serde(default)]
    burn_dust: bool,
}

/// One recover at a time. Two overlapping sweeps would close the same accounts
/// twice and pay for the revert.
static RECOVER_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct RecoverGuard;

impl RecoverGuard {
    fn try_acquire() -> Option<Self> {
        RECOVER_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RecoverGuard)
    }
}

impl Drop for RecoverGuard {
    fn drop(&mut self) {
        RECOVER_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// POST /api/wallet/rent/recover
///
/// Closes empty token accounts and unwraps wrapped SOL. `burn_dust` also
/// destroys balances at or under the dust cap. Open-position accounts and a
/// mint with an exit in flight are left alone. Off the trade hot path.
pub(crate) async fn recover_rent(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<RecoverRequest>,
) -> impl Responder {
    let Some(_guard) = RecoverGuard::try_acquire() else {
        return HttpResponse::Conflict()
            .json(serde_json::json!({ "error": "A rent recover is already in progress" }));
    };
    match crate::services::rent::recover(app_state.get_ref(), body.burn_dust).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            tracing::warn!("recover_rent failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
