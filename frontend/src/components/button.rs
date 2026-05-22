use yew::prelude::*;

#[allow(dead_code)]
#[derive(Properties, PartialEq)]
pub struct ButtonProps {
    pub label: String,
    pub onclick: Callback<()>,
    #[prop_or_default]
    pub disabled: bool,
    #[prop_or_default]
    pub class: String,
}

#[function_component(Button)]
pub fn button(props: &ButtonProps) -> Html {
    let onclick = {
        let cb = props.onclick.clone();
        Callback::from(move |_| cb.emit(()))
    };

    html! {
        <button
            class={format!("btn {}", props.class)}
            onclick={onclick}
            disabled={props.disabled}
        >
            { &props.label }
        </button>
    }
}
