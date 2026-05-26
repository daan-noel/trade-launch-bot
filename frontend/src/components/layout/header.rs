use crate::components::{StatusButton, StatusState};
use crate::services::api::{fetch_live_mode, set_live_mode};
use crate::state::{PriceUnitAction, PriceUnitContext, PriceUnit};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;

#[function_component(Header)]
pub fn header() -> Html {
    let route = use_route::<Route>().unwrap_or(Route::NotFound);
    let live_mode = use_state(|| false);
    let price_unit = use_context::<PriceUnitContext>().expect("PriceUnitProvider must be mounted above Header");
    let price_input = use_state(|| price_unit.usd_rate.map(|v| v.to_string()).unwrap_or_default());

    {
        let live_mode = live_mode.clone();
        use_effect_with(
            (),
            move |_| {
                spawn_local(async move {
                    if let Ok(live) = fetch_live_mode().await {
                        live_mode.set(live);
                    }
                });
                || ()
            },
        );
    }

    let onclick_toggle = {
        let live_mode = live_mode.clone();
        Callback::from(move |_| {
            let live_mode = live_mode.clone();
            spawn_local(async move {
                if let Ok(next_live) = set_live_mode(!*live_mode).await {
                    live_mode.set(next_live);
                }
            });
        })
    };

    let onclick_toggle_unit = {
        let price_unit = price_unit.clone();
        Callback::from(move |_| {
            let next = match price_unit.unit {
                PriceUnit::SOL => PriceUnit::USD,
                PriceUnit::USD => PriceUnit::SOL,
            };
            price_unit.dispatch(PriceUnitAction::SetUnit(next));
        })
    };

    let oninput_price_rate = {
        let price_input = price_input.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            price_input.set(input.value());
        })
    };

    let onclick_apply_rate = {
        let price_input = price_input.clone();
        let price_unit = price_unit.clone();
        Callback::from(move |_| {
            if let Ok(rate) = (*price_input).trim().parse::<f64>() {
                price_unit.dispatch(PriceUnitAction::SetUsdRate(Some(rate)));
            }
        })
    };

    let cls = |target: Route| -> Classes {
        if route == target {
            classes!("nav-link", "active")
        } else {
            classes!("nav-link")
        }
    };

    html! {
        <header class="topnav">
            <div class="topnav-inner">
                <Link<Route> to={Route::Home} classes={classes!("brand")}>
                    <span class="brand-icon">{"◈"}</span>
                    <span class="brand-name">{"MEME"}</span>
                    <span class="brand-suffix">{"TRADING"}</span>
                </Link<Route>>
                <nav class="nav-links">
                    <Link<Route> to={Route::Home}         classes={cls(Route::Home)}>{ "Home" }</Link<Route>>
                    <Link<Route> to={Route::Dashboard}    classes={cls(Route::Dashboard)}>{ "Dashboard" }</Link<Route>>
                    <Link<Route> to={Route::Tokens}       classes={cls(Route::Tokens)}>{ "Tokens" }</Link<Route>>
                    <Link<Route> to={Route::Transactions} classes={cls(Route::Transactions)}>{ "Transactions" }</Link<Route>>
                    <Link<Route> to={Route::Analysis}     classes={cls(Route::Analysis)}>{ "Analysis" }</Link<Route>>
                    <Link<Route> to={Route::Strategies}   classes={cls(Route::Strategies)}>{ "Strategies" }</Link<Route>>
                    <Link<Route> to={Route::Settings}     classes={cls(Route::Settings)}>{ "Settings" }</Link<Route>>
                </nav>
                <div class="topnav-right">
                    <span class="chain-badge">
                        <span class="chain-dot"></span>
                        {"SOL"}
                    </span>
                    <div class="unit-widget">
                        <button class={classes!("unit-button", price_unit.unit.label().to_lowercase())}
                            onclick={onclick_toggle_unit}>
                            { price_unit.unit.label() }
                        </button>
                        if price_unit.unit == PriceUnit::USD {
                            <div class="unit-input-group">
                                <input
                                    class="unit-input"
                                    type="number"
                                    step="0.01"
                                    min="0"
                                    value={(*price_input).clone()}
                                    oninput={oninput_price_rate}
                                    placeholder={"SOL/USD"}
                                />
                                <button class="unit-apply" onclick={onclick_apply_rate}>
                                    { "✓" }
                                </button>
                            </div>
                        }
                    </div>
                    <StatusButton
                        state={if *live_mode { StatusState::Live } else { StatusState::Dead }}
                        onclick={onclick_toggle}
                        class={"mode-button".to_string()}
                        label={Some(if *live_mode { "WS LIVE" } else { "WS DEAD" }.to_string())}
                    />
                </div>
            </div>
        </header>
    }
}
