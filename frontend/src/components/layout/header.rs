use crate::components::{StatusButton, StatusState};
use crate::services::api::{fetch_live_mode, fetch_sol_price, set_live_mode};
use crate::state::{PriceUnit, PriceUnitAction, PriceUnitContext};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;

#[function_component(Header)]
pub fn header() -> Html {
    let route = use_route::<Route>().unwrap_or(Route::NotFound);
    let live_mode = use_state(|| false);
    let price_unit =
        use_context::<PriceUnitContext>().expect("PriceUnitProvider must be mounted above Header");

    {
        let live_mode = live_mode.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                if let Ok(live) = fetch_live_mode().await {
                    live_mode.set(live);
                }
            });
            || ()
        });
    }

    {
        let price_unit = price_unit.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                if let Ok(Some(rate)) = fetch_sol_price().await {
                    price_unit.dispatch(PriceUnitAction::SetUsdRate(Some(rate)));
                }
            });
            || ()
        });
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

            if next == PriceUnit::USD {
                let price_unit = price_unit.clone();
                spawn_local(async move {
                    if let Ok(Some(rate)) = fetch_sol_price().await {
                        price_unit.dispatch(PriceUnitAction::SetUsdRate(Some(rate)));
                    }
                });
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

    let on_strategies = matches!(route, Route::Strategies | Route::StrategiesTpsl);

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
                    <div class="nav-item">
                        <span class={classes!("nav-link", on_strategies.then_some("active"))}>
                            { "Strategies" }
                            <span class="subnav-arrow">{ "▾" }</span>
                        </span>
                        <div class="subnav">
                            <Link<Route> to={Route::StrategiesTpsl}
                                classes={if route == Route::StrategiesTpsl { classes!("subnav-link", "active") } else { classes!("subnav-link") }}>
                                { "TPSL" }
                            </Link<Route>>
                        </div>
                    </div>
                    <Link<Route> to={Route::Settings}     classes={cls(Route::Settings)}>{ "Settings" }</Link<Route>>
                </nav>
                <div class="topnav-right">
                    <span class="">
                        {"SOL"}
                    </span>
                    <div class="unit-widget">
                        <button class={classes!("chain-badge", price_unit.unit.label().to_lowercase())}
                            onclick={onclick_toggle_unit}>
                            <span class="chain-dot"></span>
                            { price_unit.unit.label() }
                        </button>
                        if price_unit.unit == PriceUnit::USD {
                            <div class="unit-price-label">
                                { price_unit.usd_rate
                                    .map(|rate| format!("SOL/USD {:.2}", rate))
                                    .unwrap_or_else(|| "SOL/USD —".to_string()) }
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
