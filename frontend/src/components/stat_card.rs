use yew::prelude::*;

/// Colour variant applied to the value text.
#[derive(Clone, PartialEq, Default)]
pub enum StatVariant {
    #[default]
    Default,
    Primary, // green
    Warning, // yellow
    Danger,  // red
    Info,    // blue
    Accent,  // orange
    Muted,   // dim
}

impl StatVariant {
    pub fn class(&self) -> &'static str {
        match self {
            Self::Default => "sv",
            Self::Primary => "sv sv-primary",
            Self::Warning => "sv sv-warning",
            Self::Danger  => "sv sv-danger",
            Self::Info    => "sv sv-info",
            Self::Accent  => "sv sv-accent",
            Self::Muted   => "sv sv-muted",
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct StatCardProps {
    pub label: String,
    pub value: String,
    #[prop_or_default]
    pub variant: StatVariant,
    /// If set the value renders as an `<a>` link (opens in new tab).
    #[prop_or_default]
    pub href: Option<String>,
    /// Render the value at a larger font size (for prices, ratios, percentages).
    #[prop_or_default]
    pub large: bool,
    /// Render the value with slightly heavier font weight (600) without changing font size.
    #[prop_or_default]
    pub bold: bool,
}

#[function_component(StatCard)]
pub fn stat_card(props: &StatCardProps) -> Html {
    let value_cls = classes!(
        props.variant.class(),
        props.large.then_some("sv-lg"),
        props.bold.then_some("sv-bold"),
    );
    let value_node = if let Some(href) = &props.href {
        html! {
            <a
                class={classes!(value_cls, "sv-addr")}
                href={href.clone()}
                target="_blank"
                rel="noopener noreferrer"
            >
                { &props.value }
            </a>
        }
    } else {
        html! {
            <span class={value_cls}>{ &props.value }</span>
        }
    };

    html! {
        <div class="stat-card">
            <span class="stat-label">{ &props.label }</span>
            { value_node }
        </div>
    }
}

// ── AddrCard ──────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct AddrCardProps {
    pub label: String,
    /// Truncated address shown in the UI.
    pub short: String,
    /// Full address used for clipboard copy.
    pub full: String,
    /// Solscan explorer URL.
    pub solscan_url: String,
    /// Optional GMGN URL (e.g. token mint page).
    #[prop_or_default]
    pub gmgn_url: Option<String>,
}

#[function_component(AddrCard)]
pub fn addr_card(props: &AddrCardProps) -> Html {
    let copied = use_state(|| false);

    let on_copy = {
        let full   = props.full.clone();
        let copied = copied.clone();
        Callback::from(move |e: MouseEvent| {
            e.stop_propagation();
            let full   = full.clone();
            let copied = copied.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(win) = web_sys::window() {
                    let cb = win.navigator().clipboard();
                    let _ = wasm_bindgen_futures::JsFuture::from(cb.write_text(&full)).await;
                    copied.set(true);
                    let c2 = copied.clone();
                    gloo::timers::callback::Timeout::new(1500, move || c2.set(false)).forget();
                }
            });
        })
    };

    let is_copied = *copied;

    html! {
        <div class="stat-card addr-card">
            <span class="stat-label">{ &props.label }</span>
            <div class="addr-row">
                <span
                    class={if is_copied { "addr-text addr-copied" } else { "addr-text" }}
                    onclick={on_copy.clone()}
                    title="Click to copy"
                >
                    { if is_copied { "Copied!".to_string() } else { props.short.clone() } }
                </span>
                <div class="addr-actions">
                    <button
                        class={if is_copied { "addr-action-btn addr-action-ok" } else { "addr-action-btn" }}
                        onclick={on_copy}
                        title="Copy address"
                    >
                        { if is_copied {
                            html! {
                                <svg width="10" height="10" viewBox="0 0 24 24" fill="none">
                                    <path d="M5 13l4 4L19 7" stroke="currentColor" stroke-width="2.5"
                                          stroke-linecap="round" stroke-linejoin="round"/>
                                </svg>
                            }
                        } else {
                            html! {
                                <svg width="10" height="10" viewBox="0 0 24 24" fill="none">
                                    <rect x="9" y="9" width="13" height="13" rx="2"
                                          stroke="currentColor" stroke-width="1.8"/>
                                    <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"
                                          stroke="currentColor" stroke-width="1.8"/>
                                </svg>
                            }
                        } }
                    </button>
                    { if let Some(url) = &props.gmgn_url {
                        html! {
                            <a class="addr-action-btn addr-gmgn"
                               href={url.clone()}
                               target="_blank"
                               rel="noopener noreferrer"
                               title="Open on GMGN">
                                { "G" }
                            </a>
                        }
                    } else {
                        html! {}
                    } }
                    <a class="addr-action-btn addr-solscan"
                       href={props.solscan_url.clone()}
                       target="_blank"
                       rel="noopener noreferrer"
                       title="Open on Solscan">
                        { "S" }
                    </a>
                </div>
            </div>
        </div>
    }
}
