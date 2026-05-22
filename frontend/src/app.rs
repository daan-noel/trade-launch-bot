use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;
use crate::state::TokenProvider;

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <TokenProvider>
                <Switch<Route> render={crate::routes::switch} />
            </TokenProvider>
        </BrowserRouter>
    }
}
