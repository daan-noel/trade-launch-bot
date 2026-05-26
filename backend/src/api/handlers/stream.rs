use actix_web::{web, HttpResponse, Responder};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{models::events::InternalEvent, state::app_state::AppState};

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct StreamQuery {
    /// If provided, only stream events for this mint address.
    pub mint: Option<String>,
}

// ---------------------------------------------------------------------------
// SSE serialization
// ---------------------------------------------------------------------------

/// Serialize an `InternalEvent` into an SSE frame.
///
/// Format:
/// ```text
/// event: <type>
/// data: <json>
///
/// ```
/// Returns `None` for event variants not exposed over SSE
/// (e.g. liquidity events not yet fully utilized).
fn to_sse_frame(event: &InternalEvent, mint_filter: Option<&str>, state: &AppState) -> Option<Vec<u8>> {
    let (event_type, data) = match event {
        InternalEvent::TokenCreated(e) => {
            if mint_filter.map_or(false, |m| m != e.token.mint_address) {
                return None;
            }
            // Try to enrich with live stats from the token cache if available
            let mint = e.token.mint_address.clone();
            let live = state
                .token_cache
                .get(&mint)
                .map(|entry| {
                    let s = entry.value();
                    json!({
                        "current_price": s.current_price,
                        "volume_sol_total": s.volume_sol_total,
                        "market_cap": s.market_cap,
                        "trade_count": s.trade_count,
                        "ath_price": s.ath_price,
                        "ath_timestamp": s.ath_timestamp,
                        "last_trade_at": s.last_trade_at,
                    })
                })
                .unwrap_or(serde_json::Value::Null);

            let data = json!({
                "mint":    e.token.mint_address,
                "name":    e.token.name,
                "symbol":  e.token.symbol,
                "creator": e.token.creator_wallet,
                "bonding_curve": e.token.bonding_curve_address,
                "tx_signature": e.tx_signature,
                "slot":    e.slot,
                "timestamp": e.timestamp,
                "live":    live,
            });
            ("token_created", data)
        }
        InternalEvent::TradeExecuted(e) => {
            if mint_filter.map_or(false, |m| m != e.trade.mint_address) {
                return None;
            }
            // Enrich trade event with live token summary if available
            let mint = e.trade.mint_address.clone();
            let live = state
                .token_cache
                .get(&mint)
                .map(|entry| {
                    let s = entry.value();
                    json!({
                        "current_price": s.current_price,
                        "volume_sol_total": s.volume_sol_total,
                        "market_cap": s.market_cap,
                        "trade_count": s.trade_count,
                        "ath_price": s.ath_price,
                        "ath_timestamp": s.ath_timestamp,
                        "last_trade_at": s.last_trade_at,
                    })
                })
                .unwrap_or(serde_json::Value::Null);

            let data = json!({
                "mint":             e.trade.mint_address,
                "wallet":           e.trade.wallet_address,
                "trade_type":       e.trade.trade_type,
                "sol_amount":       e.trade.sol_amount,
                "token_amount":     e.trade.token_amount,
                "price_per_token":  e.trade.price_per_token,
                "tx_signature":     e.tx_signature,
                "slot":             e.slot,
                "timestamp":        e.timestamp,
                "live":             live,
            });
            ("trade_executed", data)
        }
        InternalEvent::LiquidityAdded(e) | InternalEvent::LiquidityRemoved(e) => {
            if mint_filter.map_or(false, |m| m != e.mint_address) {
                return None;
            }
            let event_type = if matches!(event, InternalEvent::LiquidityAdded(_)) {
                "liquidity_added"
            } else {
                "liquidity_removed"
            };
            let data = json!({
                "mint":       e.mint_address,
                "wallet":     e.wallet_address,
                "sol_amount": e.sol_amount,
                "token_amount": e.token_amount,
                "tx_signature": e.tx_signature,
                "slot":       e.slot,
                "timestamp":  e.timestamp,
            });
            (event_type, data)
        }
        // Token migration events are not emitted over SSE yet.
        InternalEvent::TokenMigrated(_) => return None,
        // CreatorActivity is internal-only — not exposed over SSE
        InternalEvent::CreatorActivityDetected(_) => return None,
    };

    let frame = format!(
        "event: {event_type}\ndata: {data}\n\n",
        data = data.to_string()
    );
    Some(frame.into_bytes())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/stream[?mint=<address>]`
///
/// Server-Sent Events stream of real-time Pump.fun activity.
///
/// Events emitted:
/// - `token_created`   — a new Pump.fun token appeared
/// - `trade_executed`  — a buy or sell on a tracked token
/// - `liquidity_added` / `liquidity_removed` — bonding curve events
///
/// Use the `?mint=` query param to subscribe to a single token only.
pub async fn stream_events(
    state: web::Data<Arc<AppState>>,
    query: web::Query<StreamQuery>,
) -> impl Responder {
    let event_rx = state.event_tx.subscribe();
    let mint_filter = query.into_inner().mint;

    // Build a Stream<Item = Result<Bytes, actix_web::Error>> using unfold.
    //
    // The broadcast Receiver is carried as the unfold state so it stays alive
    // for the duration of the connection.
    let sse_stream = stream::unfold(
        (event_rx, mint_filter, state.clone()),
        |(mut rx, mint_filter, state)| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Some(bytes) = to_sse_frame(&event, mint_filter.as_deref(), state.as_ref()) {
                            let chunk = Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(bytes));
                            return Some((chunk, (rx, mint_filter, state)));
                        }
                        // Event filtered or unsupported — wait for the next one
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // We fell behind; skip the gap and keep streaming
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Channel shut down — end the stream
                        return None;
                    }
                }
            }
        },
    );

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no")) // prevent nginx buffering
        .streaming(sse_stream)
}
