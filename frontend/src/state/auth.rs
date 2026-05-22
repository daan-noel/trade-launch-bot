use yew::prelude::*;

#[allow(dead_code)]
#[derive(Clone, PartialEq, Default)]
pub struct AuthState {
    pub is_authenticated: bool,
    pub username: Option<String>,
}

#[allow(dead_code)]
pub type AuthContext = UseStateHandle<AuthState>;

#[allow(dead_code)]
#[derive(Properties, PartialEq)]
pub struct AuthProviderProps {
    pub children: Children,
}

#[function_component(AuthProvider)]
pub fn auth_provider(props: &AuthProviderProps) -> Html {
    let state = use_state(AuthState::default);

    html! {
        <ContextProvider<AuthContext> context={state}>
            { for props.children.iter() }
        </ContextProvider<AuthContext>>
    }
}
