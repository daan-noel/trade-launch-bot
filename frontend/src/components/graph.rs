use yew::prelude::*;

#[allow(dead_code)]
#[derive(Properties, PartialEq)]
pub struct GraphProps {
    pub title: String,
    pub data: Vec<f64>,
    pub labels: Vec<String>,
}

#[function_component(Graph)]
pub fn graph(_props: &GraphProps) -> Html {
    html! {
        <div class="graph-container">
            <canvas id="graph-canvas" />
        </div>
    }
}
