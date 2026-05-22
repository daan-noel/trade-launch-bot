mod app;
mod components;
mod pages;
mod routes;
mod services;
mod state;
mod utils;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<app::App>::new().render();
}
