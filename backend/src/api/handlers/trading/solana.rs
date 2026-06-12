use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::config::constants::{DEFAULT_SLIPPAGE_BPS, SLIPPAGE_MAX_BPS};
use crate::services::clients::jupiter;
use crate::services::wallet_tokens;
use crate::state::app_state::AppState;

#[derive(Deserialize)]
pub struct BuyRequest {
    pub mint: String,
    pub sol_amount: f64,
    /// Optional: when omitted (manual buy by mint), the backend resolves it
    /// on-chain alongside the migration status.
    pub token_program_id: Option<String>,
    /// Optional per-trade slippage tolerance in basis points (100 = 1%). When
    /// omitted, falls back to the persisted default, then the built-in constant.
    pub slippage_bps: Option<u64>,
}

#[derive(Deserialize)]
pub struct SellRequest {
    pub mint: String,
    pub token_amount: u64,
    pub token_account: Option<String>,
    /// Optional per-trade slippage tolerance in basis points (100 = 1%). See
    /// [`BuyRequest::slippage_bps`].
    pub slippage_bps: Option<u64>,
}

/// Resolve the effective slippage (bps) for a trade: per-request override →
/// persisted global default → built-in constant, clamped to the hard ceiling.
fn resolve_slippage(app_state: &AppState, request: Option<u64>) -> u64 {
    request
        .or_else(|| app_state.settings().slippage_bps)
        .unwrap_or(DEFAULT_SLIPPAGE_BPS)
        .min(SLIPPAGE_MAX_BPS)
}

/// 90% of a raw token balance, rounded down (we sell 90% to leave dust headroom).
/// Widens to `u128` first: the naive `amount * 90 / 100` overflows for any
/// `amount > u64::MAX / 90` — a client-supplied raw amount can reach there — and
/// would panic in debug or silently wrap in release, selling the wrong size.
fn sell_ninety_percent(amount: u64) -> u64 {
    ((amount as u128 * 90) / 100) as u64
}

#[cfg(test)]
mod tests {
    use super::sell_ninety_percent;

    #[test]
    fn computes_ninety_percent_for_normal_values() {
        assert_eq!(sell_ninety_percent(0), 0);
        assert_eq!(sell_ninety_percent(100), 90);
        assert_eq!(sell_ninety_percent(1_000_000_000), 900_000_000);
        // Rounds down.
        assert_eq!(sell_ninety_percent(9), 8);
    }

    #[test]
    fn does_not_overflow_near_u64_max() {
        // The old `amount * 90` would panic (debug) / wrap (release) for any
        // amount above this threshold; the u128 widening keeps it exact.
        let threshold = u64::MAX / 90;
        assert_eq!(sell_ninety_percent(threshold), (threshold as u128 * 90 / 100) as u64);

        // u64::MAX itself must not panic and stays below the input.
        let got = sell_ninety_percent(u64::MAX);
        let expected = (u64::MAX as u128 * 90 / 100) as u64;
        assert_eq!(got, expected);
        assert!(got < u64::MAX);
    }
}

/// POST /api/solana/wallet/buy
pub async fn manual_buy(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<BuyRequest>,
) -> impl Responder {
    // Resolve routing facts (creator, token program, migration status) from
    // chain — the source of truth, so a typed-in mint the cache has never seen,
    // a just-migrated token, or a Token-2022 mint all route correctly.
    let routing = match app_state.trader.resolve_buy_routing(&body.mint).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("manual_buy: resolve_buy_routing failed for {}: {e}", body.mint);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Could not resolve token: {e}") }));
        }
    };

    // Caller-supplied token program wins; otherwise use the on-chain owner.
    let token_program_id = body
        .token_program_id
        .clone()
        .unwrap_or(routing.token_program_id);
    let sol_amount = body.sol_amount;
    let slippage_bps = resolve_slippage(&app_state, body.slippage_bps);

    let buy_result = if routing.is_migrated {
        // Migrated → PumpSwap AMM (canonical pool derived).
        // Mayhem tokens never migrate, so the AMM path needs no mayhem handling.
        app_state
            .trader
            .amm_buy(&body.mint, &token_program_id, sol_amount, None, Some(slippage_bps))
            .await
    } else {
        // Still on the bonding curve.
        app_state
            .trader
            .buy_token(&body.mint, &routing.creator, &token_program_id, sol_amount, Some(slippage_bps))
            .await
    };
    match buy_result {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_buy failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// POST /api/solana/wallet/sell
pub async fn manual_sell(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<SellRequest>,
) -> impl Responder {
    let token_account_override = body.token_account.as_deref();
    let sell_amount = sell_ninety_percent(body.token_amount);

    // Resolve routing live on-chain — same as manual_buy. The in-memory
    // token_cache can be stale or empty for a freshly-migrated token, and a
    // false `is_migrated` would misroute a migrated sell to the bonding curve,
    // which the on-chain program rejects with BondingCurveComplete (6005).
    let routing = match app_state.trader.resolve_buy_routing(&body.mint).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("manual_sell: resolve_buy_routing failed for {}: {e}", body.mint);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Could not resolve token: {e}") }));
        }
    };

    // is_cashback only gates the bonding-curve sell's cashback account; the AMM
    // reads it from the pool on-chain. Pull it from the cache when present (the
    // Ref is dropped within this statement, never held across an await).
    let is_cashback = app_state
        .token_cache
        .get(&body.mint)
        .map(|e| e.token.is_cashback_enabled)
        .unwrap_or(false);

    let slippage_bps = resolve_slippage(&app_state, body.slippage_bps);

    let sell_result = if routing.is_migrated {
        app_state
            .trader
            .amm_sell(
                &body.mint,
                sell_amount,
                &routing.token_program_id,
                None,
                token_account_override,
                Some(slippage_bps),
                0,
                // Manual API sell: block on RPC confirm (no feed loop here).
                true,
            )
            .await
    } else {
        app_state
            .trader
            .sell_token(
                &body.mint,
                sell_amount,
                Some(&routing.creator),
                is_cashback,
                token_account_override,
                Some(slippage_bps),
            )
            .await
    };
    match sell_result {
        Ok(success) => HttpResponse::Ok().json(serde_json::json!({ "success": success })),
        Err(e) => {
            tracing::warn!("manual_sell failed mint={}: {e}", body.mint);
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// GET /api/solana/wallet/tokens
///
/// Returns all non-zero token accounts held by the trader's wallet,
/// enriched with symbol (from token cache) and current USD price (Jupiter).
pub async fn get_wallet_tokens(app_state: web::Data<Arc<AppState>>) -> impl Responder {
    match wallet_tokens::list_enriched(app_state.get_ref()).await {
        Ok(enriched) => HttpResponse::Ok().json(enriched),
        Err(e) => {
            tracing::warn!("get_wallet_tokens failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// GET /api/solana/wallet/tokens/{mint}
///
/// Single enriched holding for the trader's own wallet, or `null` if not held.
/// A cheap one-mint read (one RPC + one Jupiter price) used to confirm a
/// balance change after a manual trade without re-scanning/re-pricing the
/// whole wallet.
pub async fn get_wallet_token(
    app_state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let mint = path.into_inner();
    match wallet_tokens::enrich_one(app_state.get_ref(), &mint).await {
        Ok(holding) => HttpResponse::Ok().json(holding),
        Err(e) => {
            tracing::warn!("get_wallet_token failed mint={mint}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[derive(Deserialize)]
pub struct PricesQuery {
    /// Comma-separated mint addresses.
    pub ids: String,
}

/// GET /api/solana/prices?ids=mint1,mint2,...
///
/// Live Jupiter prices for a set of mints, decoupled from the (slow,
/// RPC-bound) wallet balance read so the wallet table can refresh values on a
/// short poll without re-scanning the chain. Returns a
/// `{ mint: { price_usd, liquidity, price_change_24h, token_created_at } }`
/// map; mints Jupiter doesn't price are simply absent.
pub async fn get_prices(query: web::Query<PricesQuery>) -> impl Responder {
    let mints: Vec<String> = query
        .ids
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    match jupiter::fetch_prices(&mints).await {
        Ok(prices) => HttpResponse::Ok().json(prices),
        Err(e) => {
            tracing::warn!("get_prices failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// GET /api/solana/wallet/{wallet}/token/{mint}
///
/// Returns the on-chain token balance for the given wallet and mint,
/// queried directly from Solana — does not touch the local database.
pub async fn get_wallet_token_balance(
    app_state: web::Data<Arc<AppState>>,
    path: web::Path<(String, String)>,
) -> impl Responder {
    let (wallet, mint) = path.into_inner();
    match app_state.trader.get_token_balance(&wallet, &mint).await {
        Ok(balance) => HttpResponse::Ok().json(balance),
        Err(e) => {
            tracing::warn!("get_wallet_token_balance failed wallet={wallet} mint={mint}: {e}");
            HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
