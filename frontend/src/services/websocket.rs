use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{EventSource, MessageEvent};
use yew::UseReducerHandle;

use crate::services::api::API_BASE;
use crate::state::token::{TokenAction, TokenState};
use crate::state::transactions::{LiveTrade, TransactionAction, TransactionState};

fn sse_url() -> String {
    format!("{}/api/stream", API_BASE)
}

/// Opens an SSE connection to the backend and wires up named-event listeners
/// for trade frames and forwards them to the transactions reducer.
pub fn connect_sse(dispatch: UseReducerHandle<TransactionState>) -> EventSource {
    let url = sse_url();
    let es = EventSource::new(&url).expect("EventSource::new failed");

    let d = dispatch.clone();
    let on_trade = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Some(raw) = e.data().as_string() {
            match serde_json::from_str::<LiveTrade>(&raw) {
                Ok(trade) => d.dispatch(TransactionAction::Prepend(trade)),
                Err(err) => log::warn!("SSE trade_executed parse error: {err}"),
            }
        }
    });

    es.add_event_listener_with_callback("trade_executed", on_trade.as_ref().unchecked_ref())
        .expect("addEventListener(trade_executed) failed");
    on_trade.forget();

    es
}

/// Opens an SSE connection and listens for token-related events. When a token
/// event is received we build a `TokenDiff` from the SSE payload and dispatch
/// `TokenAction::UpdateTokens` so the client can apply incremental updates
/// without fetching full details.
pub fn connect_sse_tokens(dispatch: UseReducerHandle<TokenState>) -> EventSource {
    let url = sse_url();
    let es = EventSource::new(&url).expect("EventSource::new failed");

    // token_created, trade_executed, liquidity_added, liquidity_removed
    let d = dispatch.clone();
    let on_token_event = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Some(raw) = e.data().as_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(mint) = v.get("mint").and_then(|m| m.as_str()) {
                    let mut diff = crate::state::token::TokenDiff {
                        mint_address: mint.to_string(),
                        name: None,
                        symbol: None,
                        current_price: None,
                        ath_price: None,
                        ath_timestamp: None,
                        market_cap: None,
                        volume_sol_delta: None,
                        trade_count_delta: None,
                        last_trade_at: None,
                        initial_buy_sol: None,
                        initial_supply_token: None,
                        cu_limit: None,
                        cu_price: None,
                        is_migrated: None,
                    };

                    let evt_type = e.type_();
                    match evt_type.as_str() {
                        "trade_executed" => {
                            diff.current_price = v.get("price_per_token").and_then(|p| p.as_f64());
                            diff.volume_sol_delta = v.get("sol_amount").and_then(|s| s.as_f64());
                            diff.trade_count_delta = Some(1);
                            diff.last_trade_at = v
                                .get("timestamp")
                                .and_then(|t| t.as_str().map(|s| s.to_string()));
                        }
                        "token_created" => {
                            diff.name = v
                                .get("name")
                                .and_then(|n| n.as_str().map(|s| s.to_string()));
                            diff.symbol = v
                                .get("symbol")
                                .and_then(|s| s.as_str().map(|s| s.to_string()));
                            diff.last_trade_at = v
                                .get("timestamp")
                                .and_then(|t| t.as_str().map(|s| s.to_string()));
                        }
                        "liquidity_added" | "liquidity_removed" => {
                            diff.volume_sol_delta = v.get("sol_amount").and_then(|s| s.as_f64());
                            diff.last_trade_at = v
                                .get("timestamp")
                                .and_then(|t| t.as_str().map(|s| s.to_string()));
                        }
                        _ => {}
                    }

                    d.dispatch(TokenAction::UpdateTokens {
                        diffs: vec![diff],
                        total: None,
                    });
                }
            }
        }
    });

    for evt in &[
        "token_created",
        "trade_executed",
        "liquidity_added",
        "liquidity_removed",
    ] {
        es.add_event_listener_with_callback(evt, on_token_event.as_ref().unchecked_ref())
            .unwrap_or_else(|_| panic!("addEventListener({}) failed", evt));
    }
    on_token_event.forget();

    es
}
