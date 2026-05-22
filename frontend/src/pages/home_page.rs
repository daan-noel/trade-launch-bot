use yew::prelude::*;

use crate::components::Header;

#[function_component(HomePage)]
pub fn home_page() -> Html {
    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="page-hero">
                    <h1 class="hero-title">{"Meme Trading"}</h1>
                    <p class="hero-sub">{"Real-time pump.fun trade monitor"}</p>
                </div>
            </main>
        </div>
    }
}
