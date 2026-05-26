use yew::prelude::*;
use yew_router::prelude::*;

use crate::routes::Route;
use crate::state::{PriceUnitProvider, TokenProvider};

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <PriceUnitProvider>
                <TokenProvider>
                    <Switch<Route> render={crate::routes::switch} />
                </TokenProvider>
            </PriceUnitProvider>
        </BrowserRouter>
    }
}
