//! Wallet-centric analysis reads (the Trader Analysis page). Given a wallet
//! address, returns the FULL token record for every mint it traded in a recent
//! window, merged with the wallet's per-mint interaction stats AND a
//! reconstructed avg-cost PnL (`kernel::wallet_mint_pnl`) — the row set the
//! page's token table renders (all token fields + wallet columns) and drives its
//! synced charts grid + PnL analytics panel from.
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
use trading_core::storage::repositories::trade_repo::WalletTradedMint;
use trading_core::strategies::kernel::wallet_mint_pnl;

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
/// the wallet's interaction stats AND reconstructed PnL on that mint. Wallet
/// fields are prefixed to avoid colliding with `TokenSummary::last_trade_at` (the
/// token's *global* last trade) under `serde(flatten)`.
///
/// The PnL fields are computed by [`wallet_mint_pnl`] (`kernel.rs` — shared with
/// the strategy cost model, so the pump.fun fee constant can't drift between
/// "our own positions" and "a wallet we're studying"). See its doc comments for
/// exactly what each figure means and how `partial_data` should be read.
#[derive(Serialize)]
struct WalletTokenRow {
    #[serde(flatten)]
    token: TokenSummary,
    /// The wallet's first trade on this mint *within the window* — a hold-
    /// duration proxy at this per-mint grain (not a true per-episode hold: a
    /// wallet that re-entered the mint several times has its FIRST entry here).
    wallet_first_trade_at: DateTime<Utc>,
    /// The wallet's most-recent trade on this mint — the table's default sort.
    wallet_last_trade_at: DateTime<Utc>,
    /// The wallet's buy/sell counts on this mint within the window.
    wallet_buy_count: i64,
    wallet_sell_count: i64,
    /// Σ SOL bought/sold in the window (recorded curve-side amount, pre-fee).
    wallet_buy_sol: f64,
    wallet_sell_sol: f64,
    /// SOL per raw token unit (same convention as `TokenSummary.current_price`),
    /// `null` when that side has no legs in the window.
    wallet_avg_buy_price: Option<f64>,
    wallet_avg_sell_price: Option<f64>,
    /// `buy_token_amount - sell_token_amount` (raw units). Positive = still
    /// holding a bag; negative only when `wallet_partial_data` is true.
    wallet_net_token_amount: i64,
    /// Realized PnL on the matched (closed) portion, gross of the pump.fun fee.
    wallet_realized_pnl_sol: f64,
    /// Same, net of the measured pump.fun protocol fee (no tip/priority charge).
    wallet_realized_pnl_sol_net_of_fee: f64,
    /// `realized_pnl_sol` as a % of the matched cost basis; `null` when there's
    /// no cost basis to divide by (no buys in the window).
    wallet_realized_pnl_pct: Option<f64>,
    /// Mark-to-market PnL on the still-open bag (uses the token's current
    /// price); `null` when there's no open bag or the price is unknown.
    wallet_unrealized_pnl_sol: Option<f64>,
    /// `realized_pnl_sol + unrealized_pnl_sol` — the one ranking number.
    wallet_total_pnl_sol: f64,
    /// `net_token_amount > 0` — still holding some of this mint.
    wallet_is_open: bool,
    /// The wallet sold more than it bought in the window (its opening buy
    /// predates `since`) — every PnL figure above is a partial estimate.
    wallet_partial_data: bool,
}

/// `GET /api/wallets/:wallet/tokens` — full token rows for every mint the wallet
/// traded in the last `days`, most-recent-trade first, capped at `limit`, each
/// merged with the wallet's interaction stats + reconstructed PnL (see
/// [`WalletTokenRow`]). Both buys and sells count (a mint the wallet only exited
/// in the window still shows).
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
        .filter_map(|t| by_mint.remove(&t.mint_address).map(|token| wallet_token_row(token, t)))
        .collect();

    HttpResponse::Ok().json(out)
}

/// Build one response row: the token's `current_price` feeds `wallet_mint_pnl`'s
/// mark-to-market of any still-open bag; the fee-adjusted, matched-cost-basis PnL
/// itself is computed once in [`kernel::wallet_mint_pnl`], never re-derived here.
fn wallet_token_row(token: TokenSummary, t: WalletTradedMint) -> WalletTokenRow {
    let pnl = wallet_mint_pnl(
        t.buy_sol,
        t.sell_sol,
        t.buy_token_amount,
        t.sell_token_amount,
        token.current_price,
    );
    WalletTokenRow {
        token,
        wallet_first_trade_at: t.first_trade_at,
        wallet_last_trade_at: t.last_trade_at,
        wallet_buy_count: t.buy_count,
        wallet_sell_count: t.sell_count,
        wallet_buy_sol: t.buy_sol,
        wallet_sell_sol: t.sell_sol,
        wallet_avg_buy_price: pnl.avg_buy_price,
        wallet_avg_sell_price: pnl.avg_sell_price,
        wallet_net_token_amount: pnl.net_token_amount,
        wallet_realized_pnl_sol: pnl.realized_pnl_sol,
        wallet_realized_pnl_sol_net_of_fee: pnl.realized_pnl_sol_net_of_fee,
        wallet_realized_pnl_pct: pnl.realized_pnl_pct,
        wallet_unrealized_pnl_sol: pnl.unrealized_pnl_sol,
        wallet_total_pnl_sol: pnl.total_pnl_sol,
        wallet_is_open: pnl.is_open,
        wallet_partial_data: pnl.partial_data,
    }
}
