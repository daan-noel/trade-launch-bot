use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub enum StatusState {
    Live,
    Dead,
}

impl StatusState {
    pub fn as_class(&self) -> &'static str {
        match self {
            StatusState::Live => "live",
            StatusState::Dead => "dead",
        }
    }

    pub fn as_text(&self) -> &'static str {
        match self {
            StatusState::Live => "LIVE",
            StatusState::Dead => "DEAD",
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct StatusButtonProps {
    pub state: StatusState,
    pub onclick: Callback<()>,
    #[prop_or_default]
    pub disabled: bool,
    #[prop_or_default]
    pub class: String,
    #[prop_or_default]
    pub label: Option<String>,
}

#[function_component(StatusButton)]
pub fn status_button(props: &StatusButtonProps) -> Html {
    let onclick = {
        let cb = props.onclick.clone();
        Callback::from(move |_| cb.emit(()))
    };
    let label = props.label.as_deref().unwrap_or(props.state.as_text());

    html! {
        <button
            class={classes!("status-button", props.state.as_class(), props.class.clone())}
            {onclick}
            disabled={props.disabled}
        >
            { label }
        </button>
    }
}
