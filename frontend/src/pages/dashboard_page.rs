use yew::prelude::*;

use crate::components::{trade_row, AppTable, Header, RowCells};
use crate::services::websocket::connect_sse;
use crate::state::transactions::TransactionState;

const HEADERS: &[&str] = &[
    "Mint",
    "Side",
    "Wallet",
    "SOL",
    "Tokens",
    "Price (SOL)",
    "Signature",
    "Slot",
    "Time (UTC)",
];

#[function_component(DashboardPage)]
pub fn dashboard_page() -> Html {
    let state = use_reducer(TransactionState::default);

    // Open SSE connection once on mount; close it on unmount.
    {
        let state = state.clone();
        use_effect_with((), move |_| {
            let es = connect_sse(state);
            move || {
                es.close();
            }
        });
    }

    let event_count = state.events.len();
    let rows = state
        .events
        .iter()
        .map(trade_row)
        .collect::<Vec<RowCells>>();
    let headers = HEADERS
        .iter()
        .map(|&h| AttrValue::Static(h))
        .collect::<Vec<_>>();

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="section-header">
                    <h2 class="section-title">{"Live Trades"}</h2>
                    <span class="live-badge">
                        <span class="live-dot"></span>
                        {"LIVE"}
                    </span>
                </div>

                if event_count == 0 {
                    <p class="loading">{ "Waiting for live trades from stream…" }</p>
                } else {
                    <AppTable
                        headers={headers}
                        rows={rows}
                        label="trades"
                        empty_message="Waiting for live trades…"
                    />
                }
            </main>
        </div>
    }
}
