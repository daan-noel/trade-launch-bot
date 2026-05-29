pub mod dashboard;
pub mod home;
pub mod settings;
pub mod strategies;
pub mod transactions;

use yew::prelude::*;
use yew_router::prelude::*;
use yew_router::components::Redirect;

use crate::pages::{AnalysisPage, DashboardPage, HomePage, TpslPage, TokensPage, TransactionsPage, WalletPage};

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/dashboard")]
    Dashboard,
    #[at("/tokens")]
    Tokens,
    #[at("/transactions")]
    Transactions,
    #[at("/analysis")]
    Analysis,
    #[at("/settings")]
    Settings,
    #[at("/strategies/tpsl")]
    StrategiesTpsl,
    #[at("/strategies")]
    Strategies,
    #[at("/wallet")]
    Wallet,
    #[not_found]
    #[at("/404")]
    NotFound,
}

pub fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <HomePage /> },
        Route::Dashboard => html! { <DashboardPage /> },
        Route::Tokens => html! { <TokensPage /> },
        Route::Transactions => html! { <TransactionsPage /> },
        Route::Analysis => html! { <AnalysisPage /> },
        Route::Settings => html! { <crate::routes::settings::SettingsRoute /> },
        Route::StrategiesTpsl => html! { <TpslPage /> },
        Route::Strategies => html! { <Redirect<Route> to={Route::StrategiesTpsl} /> },
        Route::Wallet => html! { <WalletPage /> },
        Route::NotFound => html! { <h1>{ "404 - Not Found" }</h1> },
    }
}
