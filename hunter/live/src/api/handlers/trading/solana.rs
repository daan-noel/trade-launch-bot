use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use trading_core::config::constants::{resolve_sell_slippage_bps, validate_slippage_bps};
use trading_core::models::wallet::validate_solana_address;
use crate::services::clients::jupiter;
use crate::services::wallet_tokens;
use crate::state::deploy_state::DeployState;

/// Max mints accepted per `get_prices` request. The `ids` list is fanned into a
/// single Jupiter URL, so an unbounded list is a cheap amplification vector —
/// cap it (extra ids are dropped).
const MAX_PRICE_IDS: usize = 100;

/// Manual-sell position reconcile: retry budget and spacing while the on-chain
/// sell indexes into the `trades` feed (so the closed position records the real
/// exit price). ~6 attempts × 2s comfortably covers LaserStream index lag; the
/// per-strategy boot/maintenance reaper backstops anything still unresolved.
const RECONCILE_MAX_ATTEMPTS: u32 = 6;
const RECONCILE_RETRY_SECS: u64 = 2;

#[derive(Deserialize)]
pub struct SellRequest {
    pub mint_address: String,
    /// Legacy hint of the wallet's token account for `mint` (row-triggered "Sell
    /// All"). No longer used for routing: `manual_sell` now enumerates EVERY
    /// account the wallet holds for the mint and sweeps each, so a single-account
    /// hint can't represent the multi-account case it was orphaning. Kept for wire
    /// back-compat (the field is simply ignored). The amount is never taken from
    /// the client — each account's live on-chain balance is sold in full.
    #[allow(dead_code)]
    pub token_account: Option<String>,
    /// Optional per-trade slippage tolerance in basis points (100 = 1%), used
    /// **exactly as sent**. Omitted ⇒ the persisted sell setting, which itself
    /// defaults to no floor (min_out = 1, "sell all"). `0` is a 400 — omit the
    /// field to ask for no floor.
    pub slippage_bps: Option<u64>,
}

/// `resolve_buy_routing` with a tiny retry (B6). It does one `getMultipleAccounts`;
/// a transient RPC blip would otherwise fail the whole manual trade with a 400/500.
/// Off the hot path (manual handlers only), bounded to 2 attempts with a short
/// backoff. Returns the last error untouched so the caller's status mapping is
/// unchanged on a real (non-transient) failure.
async fn resolve_buy_routing_retry(
    trader: &pump_trader::PumpFunTrader,
    mint: &str,
) -> Result<pump_trader::BuyRouting, pump_trader::TradeError> {
    const ATTEMPTS: usize = 2;
    let mut last_err = None;
    for attempt in 1..=ATTEMPTS {
        match trader.resolve_buy_routing(mint).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                if attempt < ATTEMPTS {
                    tracing::debug!("resolve_buy_routing attempt {attempt} failed for {mint}: {e}; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.expect("loop runs at least once"))
}

fn resolve_sell_slippage(app_state: &DeployState, request: Option<u64>) -> Option<u64> {
    resolve_sell_slippage_bps(app_state.settings().sell_slippage_bps, request)
}

/// Max passes the "Sell All" clear loop makes before giving up: each pass reads
/// the live balance and sells all of it, so one pass normally clears the account
/// (the inner sell already confirms + retries). Extra passes only catch a partial
/// fill; once unsellable sub-threshold dust is all that remains, retrying just
/// re-pays fees on a sell that can't clear it, so the loop stops and the close
/// no-ops on the leftover dust.
const SELL_ALL_MAX_PASSES: usize = 3;


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
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<SellRequest>,
) -> impl Responder {
    // Validate the mint format before any RPC (same as manual_buy).
    if let Err(e) = validate_solana_address(&body.mint_address) {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": format!("invalid mint: {e}") }));
    }
    // Same door policy as the settings write: the value is honored literally, so a
    // literal `0` (revert on any movement) can only be a mistake — omit the field
    // for no floor.
    if let Err(e) = validate_slippage_bps("slippage_bps", body.slippage_bps) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }

    // N2 fix: claim the engine's per-mint exit lock for the whole sweep so a
    // wallet-level "Sell All" can never race a concurrent bot/orphan exit on the
    // same mint (both firing → one lands, the other reverts into an empty wallet).
    let Some(_mint_guard) = app_state.inflight.try_begin_exit_mint(&body.mint_address) else {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "A bot exit is in progress for this mint — retry once it settles"
        }));
    };

    let slippage = resolve_sell_slippage(&app_state, body.slippage_bps);

    // Enumerate EVERY token account the wallet holds for this mint — not just the
    // first. Solana can spawn multiple non-canonical accounts for one mint (the
    // create-with-seed template pool used by the snipe buy path), and a "sell all"
    // that drained only the first account silently orphaned funds in the rest.
    // Sweep them all. Retried (B6) so a transient RPC blip doesn't fail the trade.
    let accounts = {
        const ATTEMPTS: usize = 2;
        let mut last: Result<Vec<(_, _)>, _> = Ok(Vec::new());
        for attempt in 1..=ATTEMPTS {
            last = app_state
                .trader
                .get_all_token_accounts_for_mint(&body.mint_address)
                .await;
            if last.is_ok() {
                break;
            }
            if attempt < ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
        match last {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("manual_sell: account enumeration failed for {}: {e}", body.mint_address);
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": format!("Could not list token accounts: {e}") }));
            }
        }
    };

    // No accounts at all → nothing to sell; treat as already cleared (idempotent).
    if accounts.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!({ "success": true }));
    }

    // Per-account clear loop. For each account, read its live balance and sell all
    // of it, bounded by SELL_ALL_MAX_PASSES (one pass normally clears; extra passes
    // mop up a partial fill). Each sell passes the account's OWN address as the
    // override so the right account is drained, and each cleared account is closed
    // for rent reclaim. `sold_any` lets a sell that left sub-threshold dust still
    // report success; a hard error before any sell landed surfaces as a 500.
    //
    // A stale-creator revert (2006) is self-healed inside `sell_token`/`amm_sell`
    // (refresh the creator/coin_creator cache + resend once, `confirm=true`) —
    // see `pump_trader`'s `sell.rs`/`amm.rs` — so it never needs special-casing
    // here; a pass only sees it as a plain `Err` if that internal heal itself
    // couldn't fix it (unchanged creator, or the refresh RPC failed).
    let mut sold_any = false;
    let mut last_err: Option<String> = None;
    for (account_pk, initial_amount) in &accounts {
        // Skip an account already empty at enumeration time — nothing to sell, but
        // still close it below for rent reclaim.
        if *initial_amount > 0 {
            let account_str = account_pk.to_string();
            for pass in 0..SELL_ALL_MAX_PASSES {
                // Resolve routing live on-chain EACH pass (B3). The in-memory
                // token_cache can be stale/empty for a freshly-migrated token, and a
                // token can migrate mid-loop — a once-resolved `is_migrated=false`
                // would route every pass to the bonding curve, which the program
                // rejects with BondingCurveComplete (6005). Re-resolving per pass also
                // keeps `is_cashback` fresh. Retried (B6) so a transient
                // getMultipleAccounts blip doesn't fail the trade.
                let routing = match resolve_buy_routing_retry(&app_state.trader, &body.mint_address).await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("manual_sell: resolve_buy_routing failed for {}: {e}", body.mint_address);
                        last_err = Some(format!("Could not resolve token: {e}"));
                        break;
                    }
                };
                // is_cashback gates only the bonding-curve sell's
                // user_volume_accumulator account; the AMM reads cashback from the
                // pool on-chain. Take it from the live routing read (B1) — NOT the
                // token_cache, which is empty/stale for a manual sell with no buy this
                // session and caused the Custom(6024) missing-UVA revert.
                let is_cashback = routing.cashback_enabled;

                // Read THIS account's live balance directly (not the cache-first
                // single-account resolver, which can't target a specific account).
                let amount = match app_state.trader.get_account_balance_raw(account_pk).await {
                    Ok(a) => a,
                    Err(e) => {
                        last_err = Some(e.to_string());
                        break;
                    }
                };
                if amount == 0 {
                    break;
                }
                let sell_result = if routing.is_migrated {
                    app_state
                        .trader
                        .amm_sell(
                            &body.mint_address,
                            amount,
                            &routing.token_program_id,
                            None,
                            Some(account_str.as_str()),
                            slippage,
                            // Escalate the Jito tip per pass: a sell that lost the
                            // auction didn't land (cost nothing), so bid up rather than
                            // re-send it.
                            pass as u8,
                            // Manual API sell: block on RPC confirm (no feed loop here).
                            true,
                        )
                        .await
                        // `amm_sell` returns the submitted signature for the feed path;
                        // this manual handler only needs "did it submit", so collapse to
                        // bool to match the `sell_token` branch.
                        .map(|sig| sig.is_some())
                } else {
                    app_state
                        .trader
                        .sell_token(
                            &body.mint_address,
                            amount,
                            Some(&routing.creator),
                            is_cashback,
                            Some(account_str.as_str()),
                            slippage,
                        )
                        .await
                };
                match sell_result {
                    Ok(_) => sold_any = true,
                    Err(e) => {
                        tracing::warn!(
                            "manual_sell failed mint={} account={account_str} pass={pass}: {e}",
                            body.mint_address
                        );
                        last_err = Some(e.to_string());
                        break;
                    }
                }
            }
        }

        // Rent reclaim (off the hot path): close this now-empty account to recover
        // its ~0.002 SOL rent. Fire-and-forget — recent-blockhash, preflight on, no
        // Jito tip — so it never blocks the response and cheaply no-ops if
        // sub-threshold dust kept the account funded. Mirrors the strategy exit's
        // close step (see tpsl_sniper_* `sell_and_close_position`).
        let trader = app_state.trader.clone();
        let mint = body.mint_address.clone();
        let account_str = account_pk.to_string();
        tokio::spawn(async move {
            if let Err(err) = trader.close_token_account(&mint, Some(account_str.as_str())).await {
                tracing::debug!(mint = %mint, account = %account_str, "rent-reclaim close skipped: {err}");
            }
        });
    }

    // Every account was already empty at enumeration (initial_amount==0 each) →
    // nothing to sell, but the closes above still reclaim rent. Report success.
    let all_empty = accounts.iter().all(|(_, amt)| *amt == 0);
    if !all_empty && !sold_any {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": last_err.unwrap_or_else(|| "Sell failed".to_string()),
        }));
    }

    // Position reconcile (off the response path): the bag is cleared on-chain, so
    // book any owning `Holding` real position closed (reason Manual) via the generic
    // engine's externally-cleared path — WITHOUT a fresh sell (the bag is gone; a
    // sell would only revert into an empty wallet). Else the position shows "Holding"
    // forever. A no-op for a plain wallet sell with no strategy position.
    //
    // Retries briefly so the reconcile fires only once the feed confirms the bag is
    // actually cleared (index lag) and records the real sell's exit price; the
    // engine's reaper is the ultimate backstop for anything still unresolved.
    {
        use hunter_engine::event::Fill;
        use trading_core::models::trade::TradeType;
        let state = app_state.get_ref().clone();
        let mint = body.mint_address.clone();
        tokio::spawn(async move {
            let wallet = state.trader.wallet_pubkey();
            for _ in 0..RECONCILE_MAX_ATTEMPTS {
                // Open real `Holding` positions for this mint (repo filters mode='real').
                let positions = match state.strategy_repo.find_open_by_mint(&mint, "real").await {
                    Ok(p) if p.is_empty() => break, // nothing to reconcile
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(mint = %mint, "manual-sell reconcile: open-position query failed: {e}");
                        break;
                    }
                };
                // Confirm via the trades feed that the bag is really cleared before
                // closing — the on-chain sell may not have indexed yet (index lag).
                let cleared = matches!(
                    state.trade_repo().net_token_amount_by_wallet_and_mint(&wallet, &mint).await,
                    Ok(balance) if balance <= 0
                );
                if !cleared {
                    tokio::time::sleep(std::time::Duration::from_secs(RECONCILE_RETRY_SECS)).await;
                    continue;
                }
                // Exit price/time from the most recent sell; fall back to entry if the
                // sell isn't indexed. `close_externally_cleared_position` used the same
                // `exit_price × entry_token_amount` approximation for `exit_sol`.
                let last_sell = state
                    .trade_repo()
                    .find_latest_by_wallet_mint_type(&wallet, &mint, TradeType::Sell)
                    .await
                    .ok()
                    .flatten();
                for pos in positions {
                    let (price, at) = match &last_sell {
                        Some(s) => (s.price_per_token, s.block_time),
                        None => (pos.entry_price.unwrap_or(0.0), chrono::Utc::now()),
                    };
                    let token_amount = pos.entry_token_amount.unwrap_or(0);
                    let fill = Fill { price, sol: price * token_amount as f64, token_amount, at };
                    // Engine path when registry has the row; PG book-close on miss
                    // (post-restart) so Holding never sticks after a Trade sell.
                    if state.positions.engine_id(pos.id).is_some() {
                        let _ = state.engine.reconcile_cleared(pos.id, fill).await;
                    } else {
                        use crate::strategies::engine::orphan_exit;
                        if let Err(e) = orphan_exit::book_externally_cleared_pg(
                            &state.strategy_repo,
                            pos.id,
                            fill,
                            "Manual",
                        )
                        .await
                        {
                            tracing::warn!(
                                position_id = %pos.id,
                                "manual-sell reconcile: PG book-close failed: {e}"
                            );
                        }
                    }
                }
                break;
            }
        });
    }

    HttpResponse::Ok().json(serde_json::json!({ "success": true }))
}

/// GET /api/solana/wallet/tokens
///
/// Returns all non-zero token accounts held by the trader's wallet,
/// enriched with symbol (from token cache) and current USD price (Jupiter).
pub async fn get_wallet_tokens(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
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
    app_state: web::Data<Arc<DeployState>>,
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
    app_state: web::Data<Arc<DeployState>>,
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
