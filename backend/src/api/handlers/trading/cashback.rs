use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::state::app_state::AppState;

/// Per-pot cashback figures shaped for the wallet UI. Lamports stay as raw
/// integers (well under 2^53, so JS-safe); the frontend renders SOL.
#[derive(Serialize)]
struct PotJson {
    /// "curve" or "amm".
    label: &'static str,
    /// False when the UVA account doesn't exist yet (never traded a cashback
    /// coin on that venue) — reported as all-zero, not an error.
    exists: bool,
    /// Claimable WSOL cashback (lamports) = earned − already-claimed.
    claimable_lamports: u64,
    /// Curve-only "stable" pot (separate mint, no claim path yet) — raw units.
    stable_claimable: u64,
}

#[derive(Serialize)]
struct CashbackStatusJson {
    pots: Vec<PotJson>,
    /// Sum of the WSOL-claimable across both pots (lamports).
    total_claimable_lamports: u64,
}

/// GET /api/cashback/status
///
/// Read-only cashback balance across both pots (two `getAccountInfo` calls, no
/// transaction). Off the trade hot path. The frontend caches this generously —
/// cashback accrues slowly and never needs the live price poll.
pub async fn get_cashback_status(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    match app_state.trader.cashback_status().await {
        Ok(pots) => {
            let total_claimable_lamports = pots.iter().map(|p| p.claimable()).sum();
            let body = CashbackStatusJson {
                pots: pots
                    .iter()
                    .map(|p| PotJson {
                        label: p.label,
                        exists: p.exists,
                        claimable_lamports: p.claimable(),
                        stable_claimable: p.stable_claimable(),
                    })
                    .collect(),
                total_claimable_lamports,
            };
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            tracing::warn!("get_cashback_status failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// Per-pot result of a claim attempt, shaped for the UI.
#[derive(Serialize)]
struct ClaimOutcomeJson {
    label: &'static str,
    claimable_lamports: u64,
    /// Sent signature on success; `None` if the send failed.
    signature: Option<String>,
    /// Error string when the pot's claim failed; `None` on success.
    error: Option<String>,
}

/// Guards against concurrent claim sends. `claim_cashback` reads status then
/// sends one tx per pot; two overlapping POSTs would double-send and one would
/// land as a wasted revert. A double-clicked button (or two tabs) is rejected
/// rather than paid for.
static CLAIM_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// POST /api/cashback/claim
///
/// Sweep accrued cashback from both pots back to the wallet as native SOL.
/// Off the trade hot path (recent blockhash, no nonce, no Jito tip); pots with
/// zero claimable are skipped on-trader so no empty/reverting tx is sent.
pub async fn claim_cashback(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    // Reject a second in-flight claim instead of double-sending.
    if CLAIM_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return HttpResponse::Conflict()
            .json(serde_json::json!({ "error": "A cashback claim is already in progress" }));
    }

    let result = app_state.trader.claim_cashback(true).await;
    CLAIM_IN_FLIGHT.store(false, Ordering::Release);

    match result {
        Ok(outcomes) => {
            let claimed_lamports: u64 = outcomes
                .iter()
                .filter(|o| o.err.is_none())
                .map(|o| o.claimable)
                .sum();
            let pots: Vec<ClaimOutcomeJson> = outcomes
                .iter()
                .map(|o| ClaimOutcomeJson {
                    label: o.label,
                    claimable_lamports: o.claimable,
                    signature: o.signature.clone(),
                    error: o.err.clone(),
                })
                .collect();
            HttpResponse::Ok().json(serde_json::json!({
                "claimed_lamports": claimed_lamports,
                "pots": pots,
            }))
        }
        Err(e) => {
            tracing::warn!("claim_cashback failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
