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
use trading_core::config::constants::curve_progress_pct;
use trading_core::state::core_state::CoreState;
use trading_core::storage::repositories::trade_repo::WalletTradedMint;
use trading_core::strategies::kernel::wallet_mint_pnl;

/// Query string for `GET /api/wallets/{wallet}/tokens` — the page's look-back
/// picker (a rolling day count OR an explicit `from`/`to` range) plus max tokens.
///
/// The window is EITHER rolling or explicit, never both: `from` (and optionally
/// `to`) present ⇒ `days` is ignored. `to` alone means "everything up to that
/// instant", so a past window can be read without also naming its start.
#[derive(Deserialize)]
pub struct WalletTokensParams {
    /// Rolling look-back in days (default 7), used only when `from` is absent.
    /// Clamped to 1..=90.
    #[serde(default = "default_days")]
    pub days: i64,
    /// Explicit window lower bound (RFC3339, e.g. `2026-08-18T00:00:00Z`).
    pub from: Option<DateTime<Utc>>,
    /// Explicit window upper bound, INCLUSIVE. Absent ⇒ open (up to now).
    pub to: Option<DateTime<Utc>>,
    /// Max tokens returned, recent-first. `<= 0` (the default) ⇒ every mint in
    /// the window; a positive value caps the response. Charts on the page are
    /// lazily mounted, so an unbounded token list does not fan out fetches.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_days() -> i64 {
    7
}
fn default_limit() -> i64 {
    0
}

/// Widest window a single read may scan — the same bar the rolling `days` clamp
/// enforces, applied to explicit ranges so a hand-typed `from` can't turn one
/// page load into a full-history hypertable scan. An over-long range keeps its
/// UPPER bound and moves `from` up, since the page is read end-first (rows are
/// most-recent-trade first).
const MAX_WINDOW_DAYS: i64 = 90;

/// Resolve the query's window to `(since, until)`, `until = None` meaning open.
/// Explicit bounds win over `days`; a reversed pair is swapped rather than
/// rejected (the picker can hand back either order after an edit), and the span
/// is clamped to [`MAX_WINDOW_DAYS`].
fn resolve_window(
    q: &WalletTokensParams,
    now: DateTime<Utc>,
) -> (DateTime<Utc>, Option<DateTime<Utc>>) {
    let max_span = chrono::Duration::days(MAX_WINDOW_DAYS);
    let (from, to) = match (q.from, q.to) {
        (Some(f), Some(t)) if f > t => (Some(t), Some(f)),
        pair => pair,
    };
    let until = to;
    let since = match from {
        Some(f) => f,
        // No lower bound given: hang the rolling window off the upper bound when
        // there is one, else off now, so `to` alone still reads a real window.
        None => until.unwrap_or(now) - chrono::Duration::days(q.days.clamp(1, MAX_WINDOW_DAYS)),
    };
    let end = until.unwrap_or(now);
    let since = if end - since > max_span { end - max_span } else { since };
    (since, until)
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

    // ── Position + curve depth (the first buy / last sell legs) ──────────────
    /// The wallet's first BUY in the window — the position's entry. Distinct
    /// from `wallet_first_trade_at` (first trade of *either* side): `null` when
    /// the window caught only the exit.
    wallet_entry_at: Option<DateTime<Utc>>,
    /// The wallet's last SELL in the window — the position's exit. `null` while
    /// it is still holding.
    wallet_exit_at: Option<DateTime<Utc>>,
    /// Real (non-virtual) SOL in the pool immediately BEFORE the entry buy — the
    /// curve depth the wallet bought into, its own impact backed out.
    wallet_entry_curve_sol: Option<f64>,
    /// Same depth as a percent of the graduation finish line
    /// (`PUMP_GRADUATION_REAL_SOL`). Over 100 on a post-migration pool.
    wallet_entry_curve_pct: Option<f64>,
    /// Real SOL depth immediately before the exit sell.
    wallet_exit_curve_sol: Option<f64>,
    wallet_exit_curve_pct: Option<f64>,
}

/// `GET /api/wallets/:wallet/tokens` — full token rows for every mint the wallet
/// traded in the request's window (rolling `days`, or the explicit `from`/`to`
/// range — see [`resolve_window`]), most-recent-trade first (`limit <= 0` ⇒ every
/// mint in the window; positive ⇒ capped), each merged with the wallet's
/// interaction stats + reconstructed PnL (see [`WalletTokenRow`]). Both buys and
/// sells count (a mint the wallet only exited in the window still shows).
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
    // `<= 0` ⇒ unbounded (see `wallet_traded_mints`); positive stays capped as asked.
    let limit = if query.limit <= 0 { 0 } else { query.limit };
    let (since, until) = resolve_window(&query, Utc::now());

    let traded = match state.trade_repo().wallet_traded_mints(&wallet, since, until, limit).await {
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
        wallet_entry_at: t.entry_at,
        wallet_exit_at: t.exit_at,
        wallet_entry_curve_sol: t.entry_curve_sol,
        wallet_entry_curve_pct: t.entry_curve_sol.map(curve_progress_pct),
        wallet_exit_curve_sol: t.exit_curve_sol,
        wallet_exit_curve_pct: t.exit_curve_sol.map(curve_progress_pct),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(days: i64, from: Option<&str>, to: Option<&str>) -> WalletTokensParams {
        let parse = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        WalletTokensParams {
            days,
            from: from.map(parse),
            to: to.map(parse),
            limit: 0,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-25T12:00:00Z").unwrap().with_timezone(&Utc)
    }

    /// The wire shapes the page actually sends. Guards the two things a typo
    /// here would break silently: a missing `from`/`to` must read as "no bound"
    /// (not a 400), and the RFC3339 instant the frontend builds must parse.
    #[test]
    fn query_string_parses_both_window_shapes() {
        let rolling = web::Query::<WalletTokensParams>::from_query("days=30&limit=0")
            .expect("rolling window parses")
            .into_inner();
        assert_eq!(rolling.days, 30);
        assert!(rolling.from.is_none() && rolling.to.is_none());

        let explicit = web::Query::<WalletTokensParams>::from_query(
            "limit=0&from=2026-08-18T00%3A00%3A00Z&to=2026-08-20T23%3A59%3A00Z",
        )
        .expect("explicit range parses")
        .into_inner();
        assert_eq!(explicit.days, 7, "days falls back to its default");
        assert_eq!(explicit.from.unwrap().to_rfc3339(), "2026-08-18T00:00:00+00:00");
        assert_eq!(explicit.to.unwrap().to_rfc3339(), "2026-08-20T23:59:00+00:00");
    }

    #[test]
    fn rolling_days_when_no_explicit_bounds() {
        let (since, until) = resolve_window(&params(7, None, None), now());
        assert_eq!(since, now() - chrono::Duration::days(7));
        assert!(until.is_none());
    }

    #[test]
    fn explicit_range_wins_over_days() {
        let (since, until) = resolve_window(
            &params(7, Some("2026-08-01T00:00:00Z"), Some("2026-08-03T06:30:00Z")),
            now(),
        );
        assert_eq!(since.to_rfc3339(), "2026-08-01T00:00:00+00:00");
        assert_eq!(until.unwrap().to_rfc3339(), "2026-08-03T06:30:00+00:00");
    }

    #[test]
    fn upper_bound_alone_anchors_the_rolling_window() {
        let (since, until) = resolve_window(&params(2, None, Some("2026-08-10T00:00:00Z")), now());
        assert_eq!(since.to_rfc3339(), "2026-08-08T00:00:00+00:00");
        assert_eq!(until.unwrap().to_rfc3339(), "2026-08-10T00:00:00+00:00");
    }

    #[test]
    fn reversed_bounds_are_swapped() {
        let (since, until) = resolve_window(
            &params(7, Some("2026-08-10T00:00:00Z"), Some("2026-08-01T00:00:00Z")),
            now(),
        );
        assert_eq!(since.to_rfc3339(), "2026-08-01T00:00:00+00:00");
        assert_eq!(until.unwrap().to_rfc3339(), "2026-08-10T00:00:00+00:00");
    }

    #[test]
    fn over_long_range_keeps_its_upper_bound() {
        let (since, until) = resolve_window(
            &params(7, Some("2025-01-01T00:00:00Z"), Some("2026-08-10T00:00:00Z")),
            now(),
        );
        assert_eq!(until.unwrap() - since, chrono::Duration::days(MAX_WINDOW_DAYS));
        assert_eq!(until.unwrap().to_rfc3339(), "2026-08-10T00:00:00+00:00");
    }

    #[test]
    fn open_ended_over_long_range_clamps_against_now() {
        let (since, until) = resolve_window(&params(7, Some("2025-01-01T00:00:00Z"), None), now());
        assert_eq!(now() - since, chrono::Duration::days(MAX_WINDOW_DAYS));
        assert!(until.is_none());
    }
}
