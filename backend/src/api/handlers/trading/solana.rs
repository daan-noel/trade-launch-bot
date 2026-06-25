use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::config::constants::{resolve_buy_slippage_bps, resolve_sell_slippage_bps, MAX_MANUAL_BUY_SOL};
use crate::models::wallet::validate_solana_address;
use crate::services::clients::jupiter;
use crate::services::wallet_tokens;
use crate::state::app_state::AppState;

/// Max mints accepted per `get_prices` request. The `ids` list is fanned into a
/// single Jupiter URL, so an unbounded list is a cheap amplification vector —
/// cap it (extra ids are dropped).
const MAX_PRICE_IDS: usize = 100;

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
    /// Optional hint of the wallet's token account for `mint`. Supplied by a
    /// row-triggered "Sell All" to skip a wallet scan; omitted by a manual sell
    /// (entered by mint), where the backend resolves it cache-first. The amount
    /// is NOT taken from the client — `manual_sell` reads the live on-chain
    /// balance and sells all of it.
    pub token_account: Option<String>,
    /// Optional per-trade slippage tolerance in basis points (100 = 1%). See
    /// [`BuyRequest::slippage_bps`].
    pub slippage_bps: Option<u64>,
}

fn resolve_buy_slippage(app_state: &AppState, request: Option<u64>) -> Option<u64> {
    let s = app_state.settings();
    resolve_buy_slippage_bps(s.buy_slippage_bps, s.slippage_bps, request)
}

fn resolve_sell_slippage(app_state: &AppState, request: Option<u64>) -> Option<u64> {
    resolve_sell_slippage_bps(app_state.settings().sell_slippage_bps, request)
}

/// Max passes the "Sell All" clear loop makes before giving up: each pass reads
/// the live balance and sells all of it, so one pass normally clears the account
/// (the inner sell already confirms + retries). Extra passes only catch a partial
/// fill; once unsellable sub-threshold dust is all that remains, retrying just
/// re-pays fees on a sell that can't clear it, so the loop stops and the close
/// no-ops on the leftover dust.
const SELL_ALL_MAX_PASSES: usize = 3;

/// POST /api/solana/wallet/buy
pub async fn manual_buy(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<BuyRequest>,
) -> impl Responder {
    // Validate the client-supplied spend BEFORE any on-chain work — this is real
    // SOL. Reject non-finite / non-positive (a NaN/∞ would cast to garbage
    // lamports; <= 0 wastes the tip+fee on a 0-lamport buy) and cap at the
    // per-trade ceiling so a fat-finger or hostile request can't drain the
    // wallet. The pump-trader layer guards again as a backstop.
    let sol_amount = body.sol_amount;
    if !sol_amount.is_finite() || sol_amount <= 0.0 {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "sol_amount must be a finite, positive number" }));
    }
    if sol_amount > MAX_MANUAL_BUY_SOL {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!(
                "sol_amount {sol_amount} exceeds the per-trade limit of {MAX_MANUAL_BUY_SOL} SOL"
            )
        }));
    }
    // Validate the mint format before any RPC — rejects a bad/log-injected mint
    // up front instead of wasting a `resolve_buy_routing` round-trip on it.
    if let Err(e) = validate_solana_address(&body.mint) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid mint: {e}") }));
    }

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
        .unwrap_or_else(|| routing.token_program_id.clone());
    let slippage = resolve_buy_slippage(&app_state, body.slippage_bps);

    let buy_result = if routing.is_migrated {
        // Migrated → PumpSwap AMM (canonical pool derived).
        // Mayhem tokens never migrate, so the AMM path needs no mayhem handling.
        app_state
            .trader
            .amm_buy(&body.mint, &token_program_id, sol_amount, None, slippage, true)
            .await
    } else {
        // Still on the bonding curve. Pass the pubkeys `resolve_buy_routing`
        // already parsed so the trade path doesn't re-parse them; a caller
        // override re-derives the program enum from its string.
        let token_program = match &body.token_program_id {
            Some(id) => pump_trader::TokenProgram::from_id(id),
            None => routing.token_program,
        };
        app_state
            .trader
            .buy_token(&routing.mint, &routing.creator_pubkey, token_program, sol_amount, slippage)
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

/// POST /api/solana/wallet/sell — "Sell All": clear the wallet's entire balance
/// of `mint`, then reclaim rent by closing the now-empty token account.
///
/// Off the strategy hot path (which feed-confirms via the LaserStream `trades`
/// balance), so this reads the live on-chain balance over RPC to drive the clear:
/// each pass sells 100% of the *freshly read* amount. Reading fresh is what makes
/// selling the full balance safe — the old single-shot path sold 90% only to
/// absorb a stale client-supplied amount, and the 10% it left behind kept the
/// account funded so it could never be closed. After the balance reads empty a
/// fire-and-forget `close_token_account` returns the ~0.002 SOL rent; it cheaply
/// no-ops if unsellable sub-threshold dust kept the account funded.
pub async fn manual_sell(
    app_state: web::Data<Arc<AppState>>,
    body: web::Json<SellRequest>,
) -> impl Responder {
    // Validate the mint format before any RPC (same as manual_buy).
    if let Err(e) = validate_solana_address(&body.mint) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid mint: {e}") }));
    }
    let token_account_override = body.token_account.as_deref();

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

    let slippage = resolve_sell_slippage(&app_state, body.slippage_bps);
    let wallet = app_state.trader.wallet_pubkey();

    // Clear loop: read the live balance and sell all of it, bounded by
    // SELL_ALL_MAX_PASSES. One pass normally clears (the inner sell confirms and
    // retries); extra passes only mop up a partial fill. `sold_any` lets a sell
    // that succeeded-but-left-dust still report success, while a hard error
    // before any sell landed surfaces as a 500 like the old path.
    let mut cleared = false;
    let mut sold_any = false;
    let mut last_err: Option<String> = None;
    for pass in 0..SELL_ALL_MAX_PASSES {
        let amount = match app_state.trader.get_token_balance(&wallet, &body.mint).await {
            Ok(b) => b.amount,
            Err(e) => {
                last_err = Some(e.to_string());
                break;
            }
        };
        if amount == 0 {
            cleared = true;
            break;
        }
        let sell_result = if routing.is_migrated {
            app_state
                .trader
                .amm_sell(
                    &body.mint,
                    amount,
                    &routing.token_program_id,
                    None,
                    token_account_override,
                    slippage,
                    // Escalate the Jito tip per pass: a sell that lost the auction
                    // didn't land (cost nothing), so bid up rather than re-send it.
                    pass as u8,
                    // Manual API sell: block on RPC confirm (no feed loop here).
                    true,
                )
                .await
                // `amm_sell` now returns the submitted signature for the feed path;
                // this manual handler only needs "did it submit", so collapse to bool
                // to match the `sell_token` branch.
                .map(|sig| sig.is_some())
        } else {
            app_state
                .trader
                .sell_token(
                    &body.mint,
                    amount,
                    Some(&routing.creator),
                    is_cashback,
                    token_account_override,
                    slippage,
                )
                .await
        };
        match sell_result {
            Ok(_) => sold_any = true,
            Err(e) => {
                tracing::warn!("manual_sell failed mint={} pass={pass}: {e}", body.mint);
                last_err = Some(e.to_string());
                break;
            }
        }
    }

    if !cleared && !sold_any {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": last_err.unwrap_or_else(|| "Sell failed".to_string()),
        }));
    }

    // Rent reclaim (off the hot path): close the now-empty token account to
    // recover its ~0.002 SOL rent. Fire-and-forget — recent-blockhash, preflight
    // on, no Jito tip — so it never blocks the response and cheaply no-ops if
    // sub-threshold dust kept the account funded. Mirrors the strategy exit's
    // close step (see tpsl_sniper_* `sell_and_close_position`).
    {
        let trader = app_state.trader.clone();
        let mint = body.mint.clone();
        let token_account = body.token_account.clone();
        tokio::spawn(async move {
            if let Err(err) = trader.close_token_account(&mint, token_account.as_deref()).await {
                tracing::debug!(mint = %mint, "rent-reclaim close skipped: {err}");
            }
        });
    }

    HttpResponse::Ok().json(serde_json::json!({ "success": true }))
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
    if let Err(e) = validate_solana_address(&mint) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid mint: {e}") }));
    }
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
    // Bound the fan-out (amplification guard) and drop malformed/log-injected
    // ids: `take` first so an oversized list never materializes past the cap.
    let mints: Vec<String> = query
        .ids
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(MAX_PRICE_IDS)
        .filter(|s| validate_solana_address(s).is_ok())
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
    // Validate both addresses before the RPC — skips a wasted round-trip and
    // keeps raw client input out of the logs / RPC payload.
    if let Err(e) = validate_solana_address(&wallet).and_then(|_| validate_solana_address(&mint)) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid address: {e}") }));
    }
    match app_state.trader.get_token_balance(&wallet, &mint).await {
        Ok(balance) => HttpResponse::Ok().json(balance),
        Err(e) => {
            tracing::warn!("get_wallet_token_balance failed wallet={wallet} mint={mint}: {e}");
            HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
