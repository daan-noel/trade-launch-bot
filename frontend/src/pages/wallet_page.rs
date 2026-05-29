use std::rc::Rc;

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{Column, DataTable, Header, SortKey};
use crate::services::api::{fetch_wallet_holdings, WalletHolding};
use crate::utils::format::truncate;

#[function_component(WalletPage)]
pub fn wallet_page() -> Html {
    let holdings = use_state(Vec::<WalletHolding>::new);
    let loading = use_state(|| false);
    let error = use_state(|| Option::<String>::None);

    let fetch = {
        let holdings = holdings.clone();
        let loading = loading.clone();
        let error = error.clone();
        Callback::from(move |_: ()| {
            let holdings = holdings.clone();
            let loading = loading.clone();
            let error = error.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                match fetch_wallet_holdings().await {
                    Ok(data) => { holdings.set(data); }
                    Err(e) => { error.set(Some(e)); }
                }
                loading.set(false);
            });
        })
    };

    // Fetch on mount
    {
        let fetch = fetch.clone();
        use_effect_with((), move |_| { fetch.emit(()); || () });
    }

    let on_refresh = {
        let fetch = fetch.clone();
        Callback::from(move |_: MouseEvent| fetch.emit(()))
    };

    let wallet_row_key: Rc<dyn Fn(&WalletHolding) -> String> =
        Rc::new(|r: &WalletHolding| r.mint.clone());

    let columns: Vec<Column<WalletHolding>> = vec![
        Column {
            key: "symbol",
            label: "Symbol",
            render: Rc::new(|r: &WalletHolding| {
                let sym = r.symbol.as_deref().unwrap_or("—");
                html! { <span>{ sym }</span> }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| {
                SortKey::Str(r.symbol.clone().unwrap_or_default())
            })),
            search_value: Rc::new(|r: &WalletHolding| r.symbol.clone().unwrap_or_default()),
            cell_class: None,
            sortable: true,
            default_visible: true,
            width: Some("90px"),
        },
        Column {
            key: "mint",
            label: "Mint",
            render: Rc::new(|r: &WalletHolding| {
                let short = truncate(&r.mint, 12);
                html! {
                    <a href={format!("https://gmgn.ai/sol/token/{}", r.mint)}
                        target="_blank" rel="noreferrer" class="addr">
                        { short }
                    </a>
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Str(r.mint.clone()))),
            search_value: Rc::new(|r: &WalletHolding| r.mint.clone()),
            cell_class: None,
            sortable: true,
            default_visible: true,
            width: Some("160px"),
        },
        Column {
            key: "price_usd",
            label: "Price ($)",
            render: Rc::new(|r: &WalletHolding| {
                match r.price_usd {
                    Some(p) if p < 0.0001 => html! { format!("${:.8}", p) },
                    Some(p) if p < 0.01   => html! { format!("${:.6}", p) },
                    Some(p)               => html! { format!("${:.4}", p) },
                    None                  => html! { "—" },
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.price_usd.unwrap_or(0.0)))),
            search_value: Rc::new(|r: &WalletHolding| {
                r.price_usd.map(|p| p.to_string()).unwrap_or_default()
            }),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("120px"),
        },
        Column {
            key: "ui_amount",
            label: "Amount",
            render: Rc::new(|r: &WalletHolding| html! { format!("{:.6}", r.ui_amount) }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.ui_amount))),
            search_value: Rc::new(|r: &WalletHolding| r.ui_amount.to_string()),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("140px"),
        },
        Column {
            key: "value_usd",
            label: "Value ($)",
            render: Rc::new(|r: &WalletHolding| {
                match r.value_usd {
                    Some(v) => html! { format!("${:.2}", v) },
                    None    => html! { "—" },
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.value_usd.unwrap_or(0.0)))),
            search_value: Rc::new(|r: &WalletHolding| {
                r.value_usd.map(|v| v.to_string()).unwrap_or_default()
            }),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("110px"),
        },
        Column {
            key: "liquidity",
            label: "Liquidity ($)",
            render: Rc::new(|r: &WalletHolding| {
                match r.liquidity {
                    Some(l) if l >= 1_000_000.0 => html! { format!("${:.2}M", l / 1_000_000.0) },
                    Some(l) if l >= 1_000.0     => html! { format!("${:.2}K", l / 1_000.0) },
                    Some(l)                     => html! { format!("${:.2}", l) },
                    None                        => html! { "—" },
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.liquidity.unwrap_or(0.0)))),
            search_value: Rc::new(|r: &WalletHolding| {
                r.liquidity.map(|l| l.to_string()).unwrap_or_default()
            }),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("120px"),
        },
        Column {
            key: "price_change_24h",
            label: "24h %",
            render: Rc::new(|r: &WalletHolding| {
                match r.price_change_24h {
                    Some(c) if c > 0.0 => html! {
                        <span style="color: var(--green, #22c55e)">{ format!("+{:.2}%", c) }</span>
                    },
                    Some(c) if c < 0.0 => html! {
                        <span style="color: var(--red, #ef4444)">{ format!("{:.2}%", c) }</span>
                    },
                    Some(c) => html! { format!("{:.2}%", c) },
                    None    => html! { "—" },
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.price_change_24h.unwrap_or(0.0)))),
            search_value: Rc::new(|r: &WalletHolding| {
                r.price_change_24h.map(|c| c.to_string()).unwrap_or_default()
            }),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("90px"),
        },
        Column {
            key: "token_created_at",
            label: "Token Created",
            render: Rc::new(|r: &WalletHolding| {
                match &r.token_created_at {
                    Some(ts) => html! { <span class="dim-col">{ ts.get(..19).unwrap_or(ts).replace("T", " ") }</span> },
                    None     => html! { "—" },
                }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| {
                SortKey::Str(r.token_created_at.clone().unwrap_or_default())
            })),
            search_value: Rc::new(|r: &WalletHolding| {
                r.token_created_at.clone().unwrap_or_default()
            }),
            cell_class: None,
            sortable: true,
            default_visible: true,
            width: Some("110px"),
        },
        Column {
            key: "amount",
            label: "Raw Amount",
            render: Rc::new(|r: &WalletHolding| html! { r.amount.to_string() }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.amount as f64))),
            search_value: Rc::new(|r: &WalletHolding| r.amount.to_string()),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: false,
            width: Some("140px"),
        },
        Column {
            key: "decimals",
            label: "Decimals",
            render: Rc::new(|r: &WalletHolding| html! { r.decimals.to_string() }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Num(r.decimals as f64))),
            search_value: Rc::new(|r: &WalletHolding| r.decimals.to_string()),
            cell_class: Some("num-col"),
            sortable: true,
            default_visible: true,
            width: Some("80px"),
        },
        Column {
            key: "token_program",
            label: "Program",
            render: Rc::new(|r: &WalletHolding| {
                let label = if r.token_program_id.starts_with("TokenzQdB") {
                    "Token-2022"
                } else {
                    "SPL Token"
                };
                html! { <span class="dim-col">{ label }</span> }
            }),
            sort_value: Some(Rc::new(|r: &WalletHolding| SortKey::Str(r.token_program_id.clone()))),
            search_value: Rc::new(|r: &WalletHolding| r.token_program_id.clone()),
            cell_class: None,
            sortable: true,
            default_visible: true,
            width: Some("100px"),
        },
        Column {
            key: "token_account",
            label: "Token Account",
            render: Rc::new(|r: &WalletHolding| {
                let short = truncate(&r.token_account, 12);
                html! {
                    <a href={format!("https://solscan.io/account/{}", r.token_account)}
                        target="_blank" rel="noreferrer" class="addr">
                        { short }
                    </a>
                }
            }),
            sort_value: None,
            search_value: Rc::new(|r: &WalletHolding| r.token_account.clone()),
            cell_class: None,
            sortable: false,
            default_visible: true,
            width: Some("160px"),
        },
    ];

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="tokens-page-header">
                    <div class="tokens-title-row">
                        <h2 class="tokens-page-title">{ "Wallet Holdings" }</h2>
                        <span class="token-count-badge">
                            { format!("{} tokens", holdings.len()) }
                        </span>
                        <button
                            class="dt-toolbar-btn"
                            onclick={on_refresh}
                            disabled={*loading}
                        >
                            { if *loading { "Loading…" } else { "↻ Refresh" } }
                        </button>
                    </div>
                </div>

                if let Some(err) = &*error {
                    <div class="inline-error">{ err }</div>
                }

                if *loading && holdings.is_empty() {
                    <div class="strat-state-msg">{ "Loading wallet holdings from Solana…" }</div>
                } else {
                    <DataTable<WalletHolding>
                        columns={columns}
                        rows={(*holdings).clone()}
                        row_key={wallet_row_key.clone()}
                        default_page_size={25}
                        page_size_options={vec![25usize, 50, 100]}
                        searchable={true}
                        col_filters={false}
                        col_toggle={true}
                        item_label="tokens"
                        empty_message="No token holdings found in wallet."
                    />
                }
            </main>
        </div>
    }
}
