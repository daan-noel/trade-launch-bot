use actix_web::{web, HttpResponse, Responder};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{models::ingest::SseEvent, state::app_state::AppState};

#[derive(Deserialize)]
pub struct StreamQuery {
    pub mint: Option<String>,
}

fn live_stats(state: &AppState, mint: &str) -> serde_json::Value {
    state
        .token_cache
        .get(mint)
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
        .unwrap_or(serde_json::Value::Null)
}

fn to_sse_frame(event: &SseEvent, mint_filter: Option<&str>, state: &AppState) -> Option<Vec<u8>> {
    let (event_type, data) = match event {
        SseEvent::TokenCreated {
            mint,
            tx_signature,
            slot,
            timestamp,
        } => {
            if mint_filter.is_some_and(|m| m != mint) {
                return None;
            }
            let token = state.token_cache.get(mint).map(|e| e.value().token.clone());
            let (name, symbol, creator, bonding_curve) = token
                .as_ref()
                .map(|t| {
                    (
                        t.name.as_str(),
                        t.symbol.as_str(),
                        t.creator_wallet.as_str(),
                        t.bonding_curve_address.as_deref(),
                    )
                })
                .unwrap_or(("", "", "", None));
            let data = json!({
                "mint": mint,
                "name": name,
                "symbol": symbol,
                "creator": creator,
                "bonding_curve": bonding_curve,
                "tx_signature": tx_signature,
                "slot": slot,
                "timestamp": timestamp,
                "live": live_stats(state, mint),
            });
            ("token_created", data)
        }
        SseEvent::TradeExecuted {
            mint,
            wallet,
            trade_type,
            sol_amount,
            token_amount,
            price_per_token,
            tx_signature,
            slot,
            timestamp,
        } => {
            if mint_filter.is_some_and(|m| m != mint) {
                return None;
            }
            let data = json!({
                "mint": mint,
                "wallet": wallet,
                "trade_type": trade_type,
                "sol_amount": sol_amount,
                "token_amount": token_amount,
                "price_per_token": price_per_token,
                "tx_signature": tx_signature,
                "slot": slot,
                "timestamp": timestamp,
                "live": live_stats(state, mint),
            });
            ("trade_executed", data)
        }
        SseEvent::LiquidityAdded {
            mint,
            wallet,
            sol_amount,
            token_amount,
            tx_signature,
            slot,
            timestamp,
        }
        | SseEvent::LiquidityRemoved {
            mint,
            wallet,
            sol_amount,
            token_amount,
            tx_signature,
            slot,
            timestamp,
        } => {
            if mint_filter.is_some_and(|m| m != mint) {
                return None;
            }
            let event_type = if matches!(event, SseEvent::LiquidityAdded { .. }) {
                "liquidity_added"
            } else {
                "liquidity_removed"
            };
            let data = json!({
                "mint": mint,
                "wallet": wallet,
                "sol_amount": sol_amount,
                "token_amount": token_amount,
                "tx_signature": tx_signature,
                "slot": slot,
                "timestamp": timestamp,
            });
            (event_type, data)
        }
    };

    let frame = format!(
        "event: {event_type}\ndata: {data}\n\n",
        data = data.to_string()
    );
    Some(frame.into_bytes())
}

/// `GET /api/stream[?mint=<address>]`
pub async fn stream_events(
    state: web::Data<Arc<AppState>>,
    query: web::Query<StreamQuery>,
) -> impl Responder {
    let event_rx = state.sse_tx.subscribe();
    let mint_filter = query.into_inner().mint;

    let sse_stream = stream::unfold(
        (event_rx, mint_filter, state.clone()),
        |(mut rx, mint_filter, state)| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Some(bytes) =
                            to_sse_frame(&event, mint_filter.as_deref(), state.as_ref())
                        {
                            let chunk =
                                Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(bytes));
                            return Some((chunk, (rx, mint_filter, state)));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(sse_stream)
}
