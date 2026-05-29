use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;
use crate::state::AppStateProvider;

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <AppStateProvider>
                <Switch<Route> render={crate::routes::switch} />
            </AppStateProvider>
        </BrowserRouter>
    }
}
