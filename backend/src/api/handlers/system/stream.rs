use actix_web::{web, HttpResponse, Responder};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{
    api::handlers::strategies::tpsl2_positions::PositionResponse, models::ingest::SseEvent,
    state::app_state::AppState,
};

#[derive(Deserialize)]
pub struct StreamQuery {
    pub mint: Option<String>,
}

/// A single SSE event rendered to wire bytes exactly once. Broadcast as
/// `Arc<SseFrame>` so every connection clones a ref-counted buffer instead of
/// re-running `json!` + `to_string` per subscriber (the old O(events × clients)
/// cost). `mint` carries the per-event scope so the cheap per-subscriber filter
/// (a string compare) still works without re-rendering:
///   * `Some(mint)` — deliver only to subscribers with no filter or `mint == filter`.
///   * `None`       — list-level / not-mint-scoped; deliver to everyone.
pub struct SseFrame {
    pub mint: Option<String>,
    pub bytes: web::Bytes,
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

/// Render one event to a shared `SseFrame`. Called ONCE per event by the render
/// bridge (not once per subscriber): the (single) `live_stats` cache read and
/// the JSON build happen here, off the per-connection delivery path.
fn render_sse_frame(event: &SseEvent, state: &AppState) -> SseFrame {
    let (mint_scope, event_type, data): (Option<String>, &str, serde_json::Value) = match event {
        SseEvent::TokenCreated {
            mint,
            tx_signature,
            slot,
            timestamp,
        } => {
            // Clone only the four fields the frame needs, not the whole `Token`
            // (which carries the large `instruction_labels` JSON). The shard guard
            // is dropped before `live_stats` re-reads the cache below.
            let meta = state.token_cache.get(mint).map(|e| {
                let t = &e.value().token;
                (
                    t.name.clone(),
                    t.symbol.clone(),
                    t.creator_wallet.clone(),
                    t.bonding_curve_address.clone(),
                )
            });
            let (name, symbol, creator, bonding_curve) = match &meta {
                Some((n, s, c, b)) => (n.as_str(), s.as_str(), c.as_str(), b.as_deref()),
                None => ("", "", "", None),
            };
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
            (Some(mint.clone()), "token_created", data)
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
            (Some(mint.clone()), "trade_executed", data)
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
            (Some(mint.clone()), event_type, data)
        }
        SseEvent::PaperTestFinished {
            rule_id,
            rule_name,
            run_seq,
            tokens_traded,
            timestamp,
        } => {
            // Not mint-scoped: deliver to every subscriber regardless of filter.
            let data = json!({
                "rule_id": rule_id,
                "rule_name": rule_name,
                "run_seq": run_seq,
                "tokens_traded": tokens_traded,
                "timestamp": timestamp,
            });
            (None, "paper_test_finished", data)
        }
        SseEvent::TpslRulesChanged { strategy } => {
            // Not mint-scoped: a list-level signal delivered to every subscriber.
            (None, "tpsl_rules_changed", json!({ "strategy": strategy }))
        }
        SseEvent::TpslPositionsChanged {
            strategy,
            rule_id,
            rule_snapshot,
            position,
            removed,
            open_positions,
            total_positions,
        } => {
            // Not mint-scoped: scoped to the owning rule, not a token. The changed
            // row is shipped in the `PositionResponse` wire shape so the client
            // patches one row + the badge in place rather than refetching.
            let position_json = position.as_ref().map(|p| {
                serde_json::to_value(PositionResponse::from((**p).clone()))
                    .unwrap_or(serde_json::Value::Null)
            });
            let rule_snapshot_json = rule_snapshot
                .as_deref()
                .and_then(|s| serde_json::to_value(s).ok())
                .unwrap_or(serde_json::Value::Null);
            (
                None,
                "tpsl_positions_changed",
                json!({
                    "strategy": strategy,
                    "rule_id": rule_id,
                    "rule_snapshot": rule_snapshot_json,
                    "position": position_json,
                    "removed": removed,
                    "open_positions": open_positions,
                    "total_positions": total_positions,
                }),
            )
        }
        SseEvent::SimulationProgress {
            rule_id,
            processed,
            total,
        } => {
            // Not mint-scoped: scoped to the owning rule's in-flight backtest.
            (
                None,
                "simulation_progress",
                json!({ "rule_id": rule_id, "processed": processed, "total": total }),
            )
        }
        SseEvent::SweepProgress {
            strategy_id,
            phase,
            processed,
            total,
        } => {
            // Not mint-scoped: the single-flight grouped sweep's overall progress.
            (
                None,
                "sweep_progress",
                json!({ "strategy_id": strategy_id, "phase": phase, "processed": processed, "total": total }),
            )
        }
        SseEvent::SweepFinished {
            strategy_id,
            cancelled,
        } => {
            // Not mint-scoped: terminal signal for the single-flight sweep.
            (
                None,
                "sweep_finished",
                json!({ "strategy_id": strategy_id, "cancelled": cancelled }),
            )
        }
        SseEvent::SimulationFinished { rule_id, cancelled } => {
            // Not mint-scoped: terminal signal for the rule's backtest.
            (
                None,
                "simulation_finished",
                json!({ "rule_id": rule_id, "cancelled": cancelled }),
            )
        }
        SseEvent::SwingDetectionFinished { run_id, cancelled } => {
            // Not mint-scoped: terminal signal for the "Swing Detection All" run.
            (
                None,
                "swing_detection_finished",
                json!({ "run_id": run_id, "cancelled": cancelled }),
            )
        }
    };

    let frame = format!(
        "event: {event_type}\ndata: {data}\n\n",
        data = data.to_string()
    );
    SseFrame {
        mint: mint_scope,
        bytes: web::Bytes::from(frame.into_bytes()),
    }
}

/// Long-lived bridge: drains the producer `sse_tx` broadcast, renders each event
/// to wire bytes exactly once, and re-broadcasts the shared `Arc<SseFrame>` on
/// `sse_frame_tx` for HTTP subscribers. This collapses the old per-subscriber
/// re-serialization (and per-subscriber `token_cache` reads that contended with
/// the ingest writer's `get_mut`) down to one render + one cache read per event.
/// Exits when the producer channel closes (shutdown).
pub async fn run_sse_render_bridge(state: Arc<AppState>) {
    let mut rx = state.sse_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                // Skip the render entirely when no HTTP subscriber is connected.
                if state.sse_frame_tx.receiver_count() == 0 {
                    continue;
                }
                let frame = Arc::new(render_sse_frame(&event, state.as_ref()));
                let _ = state.sse_frame_tx.send(frame);
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// `GET /api/stream[?mint=<address>]`
pub async fn stream_events(
    state: web::Data<Arc<AppState>>,
    query: web::Query<StreamQuery>,
) -> impl Responder {
    let frame_rx = state.sse_frame_tx.subscribe();
    let mint_filter = query.into_inner().mint;

    let sse_stream = stream::unfold(
        (frame_rx, mint_filter),
        |(mut rx, mint_filter)| async move {
            loop {
                match rx.recv().await {
                    Ok(frame) => {
                        // Mint-scoped frame against a mint filter: cheap string
                        // compare, no re-render. Non-scoped frames (mint == None)
                        // always pass.
                        if let (Some(ref scope), Some(ref want)) = (&frame.mint, &mint_filter) {
                            if scope != want {
                                continue;
                            }
                        }
                        let chunk =
                            Ok::<_, actix_web::Error>(frame.bytes.clone());
                        return Some((chunk, (rx, mint_filter)));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
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
