pub mod dashboard;
pub mod home;
pub mod settings;
pub mod strategies;
pub mod transactions;

use yew::prelude::*;
use yew_router::prelude::*;

use crate::pages::{AnalysisPage, DashboardPage, HomePage, TokensPage, TransactionsPage};

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
    #[at("/strategies")]
    Strategies,
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
        Route::Strategies => html! { <crate::routes::strategies::StrategiesRoute /> },
        Route::NotFound => html! { <h1>{ "404 - Not Found" }</h1> },
    }
}
