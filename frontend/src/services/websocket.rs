use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{EventSource, MessageEvent};
use yew::UseReducerHandle;

use crate::services::api::API_BASE;
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
