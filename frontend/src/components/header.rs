use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;

#[function_component(Header)]
pub fn header() -> Html {
    let route = use_route::<Route>().unwrap_or(Route::NotFound);

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
                </div>
            </div>
        </header>
    }
}
