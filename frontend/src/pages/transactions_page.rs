use yew::prelude::*;

use crate::components::{trade_row, AppTable, Header, RowCells};
use crate::services::websocket::connect_sse;
use crate::state::{transactions::TransactionState, PriceUnitContext};

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

#[function_component(TransactionsPage)]
pub fn transactions_page() -> Html {
    let tx_state = use_reducer(TransactionState::default);

    // Open SSE stream on mount; close it on unmount.
    {
        let dispatch = tx_state.clone();
        use_effect_with((), move |_| {
            let es = connect_sse(dispatch);
            move || drop(es)
        });
    }

    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above TransactionsPage");
    let event_count = tx_state.events.len();
    let rows = tx_state
        .events
        .iter()
        .map(|ev| trade_row(ev, &price_unit))
        .collect::<Vec<RowCells>>();
    let headers = HEADERS
        .iter()
        .map(|&h| {
            let label = match h {
                "SOL" => price_unit.unit_label().to_string(),
                "Price (SOL)" => format!("Price ({})", price_unit.unit_label()),
                other => other.to_string(),
            };
            AttrValue::from(label)
        })
        .collect::<Vec<_>>();

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="section-header">
                    <h2 class="section-title">{ "Live Transactions" }</h2>
                    <span class="token-count-badge">{ format!("{event_count} captured") }</span>
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
