//! Wallet-centric analysis reads (the Trader Analysis page). Given a wallet
//! address, returns the FULL token record for every mint it traded in a recent
//! window, merged with the wallet's per-mint interaction stats — the row set the
//! page's token table renders (all token fields + wallet columns) and drives its
//! synced charts grid from.
//!
//! Deliberately a **Postgres** read, not the Parquet lake: the default 7-day
//! window includes *today*, which the sealed-days-only lake lacks, so a lake
//! read would truncate the most recent (and most interesting) tokens.

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use trading_core::api::handlers::tokens::TokenSummary;
use trading_core::state::core_state::CoreState;

/// Query string for `GET /api/wallets/{wallet}/tokens`. Both knobs are the two
/// user-facing inputs on the page (look-back days + max tokens).
#[derive(Deserialize)]
pub struct WalletTokensParams {
    /// Look-back window in days (default 7). Clamped to 1..=90.
    #[serde(default = "default_days")]
    pub days: i64,
    /// Max tokens returned, recent-first (default 50). Clamped to 1..=300 to
    /// bound the fan-out of per-token chart fetches the page fires.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_days() -> i64 {
    7
}
fn default_limit() -> i64 {
    50
}

/// One row of the Trader Analysis token table: the full token record (flattened,
/// so it renders through the same frontend columns as the All Tokens table) plus
/// the wallet's interaction stats on that mint. Wallet fields are prefixed to
/// avoid colliding with `TokenSummary::last_trade_at` (the token's *global* last
/// trade) under `serde(flatten)`.
#[derive(Serialize)]
struct WalletTokenRow {
    #[serde(flatten)]
    token: TokenSummary,
    /// The wallet's most-recent trade on this mint — the table's default sort.
    wallet_last_trade_at: DateTime<Utc>,
    /// The wallet's buy/sell counts on this mint within the window.
    wallet_buy_count: i64,
    wallet_sell_count: i64,
}

/// `GET /api/wallets/:wallet/tokens` — full token rows for every mint the wallet
/// traded in the last `days`, most-recent-trade first, capped at `limit`, each
/// merged with the wallet's `{last_trade_at, buy_count, sell_count}`. Both buys
/// and sells count (a mint the wallet only exited in the window still shows).
///
/// Two indexed reads: `wallet_traded_mints` (recent-first mint set + stats) then
/// `find_list_rows_for_mints` (the same batch token projection the All Tokens /
/// `/api/tokens/batch` path uses). The wallet's recency order is re-applied after
/// the merge since `find_list_rows_for_mints` returns unspecified order.
pub async fn list_wallet_tokens(
    state: web::Data<Arc<CoreState>>,
    path: web::Path<String>,
    query: web::Query<WalletTokensParams>,
) -> impl Responder {
    let wallet = path.into_inner();
    let days = query.days.clamp(1, 90);
    let limit = query.limit.clamp(1, 300);
    let since = Utc::now() - chrono::Duration::days(days);

    let traded = match state.trade_repo().wallet_traded_mints(&wallet, since, limit).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("DB error fetching traded mints for {wallet}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }));
        }
    };

    let mints: Vec<String> = traded.iter().map(|t| t.mint_address.clone()).collect();
    let rows = match state.token_repo().find_list_rows_for_mints(&mints).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("DB error fetching token rows for {wallet}: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }));
        }
    };

    // Index the token rows by mint, then walk `traded` (recency order) so the
    // response preserves the wallet's most-recent-trade-first ordering.
    let mut by_mint: HashMap<String, TokenSummary> = rows
        .into_iter()
        .map(|r| {
            let s = TokenSummary::from(r);
            (s.mint_address.clone(), s)
        })
        .collect();

    let out: Vec<WalletTokenRow> = traded
        .into_iter()
        .filter_map(|t| {
            by_mint.remove(&t.mint_address).map(|token| WalletTokenRow {
                token,
                wallet_last_trade_at: t.last_trade_at,
                wallet_buy_count: t.buy_count,
                wallet_sell_count: t.sell_count,
            })
        })
        .collect();

    HttpResponse::Ok().json(out)
}
