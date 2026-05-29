use yew::prelude::*;

use super::{PriceUnitProvider, TpslProvider, TokenProvider};

#[derive(Properties, PartialEq)]
pub struct AppStateProviderProps {
    pub children: Children,
}

#[function_component(AppStateProvider)]
pub fn app_state_provider(props: &AppStateProviderProps) -> Html {
    html! {
        <PriceUnitProvider>
            <TokenProvider>
                <TpslProvider>
                    { for props.children.iter() }
                </TpslProvider>
            </TokenProvider>
        </PriceUnitProvider>
    }
}
