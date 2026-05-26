use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;

#[function_component(Sidebar)]
pub fn sidebar() -> Html {
    html! {
        <nav class="sidebar">
            <ul>
                <li><Link<Route> to={Route::Home}>{ "Home" }</Link<Route>></li>
                <li><Link<Route> to={Route::Dashboard}>{ "Dashboard" }</Link<Route>></li>
                <li><Link<Route> to={Route::Transactions}>{ "Transactions" }</Link<Route>></li>
                <li><Link<Route> to={Route::Analysis}>{ "Analysis" }</Link<Route>></li>
                <li><Link<Route> to={Route::Strategies}>{ "Strategies" }</Link<Route>></li>
                <li><Link<Route> to={Route::Settings}>{ "Settings" }</Link<Route>></li>
            </ul>
        </nav>
    }
}
