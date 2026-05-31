use yew::prelude::*;

#[allow(dead_code)]
#[derive(Properties, PartialEq)]
pub struct ModalProps {
    pub title: String,
    pub visible: bool,
    pub on_close: Callback<()>,
    pub children: Children,
}

#[function_component(Modal)]
pub fn modal(props: &ModalProps) -> Html {
    if !props.visible {
        return html! {};
    }

    let on_close = {
        let cb = props.on_close.clone();
        Callback::from(move |_| cb.emit(()))
    };

    html! {
        <div class="modal-overlay">
            <div class="modal">
                <div class="modal-header">
                    <h2>{ &props.title }</h2>
                    <button onclick={on_close}>{ "×" }</button>
                </div>
                <div class="modal-body">
                    { for props.children.iter() }
                </div>
            </div>
        </div>
    }
}
