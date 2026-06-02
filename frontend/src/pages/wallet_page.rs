use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{Column, DataTable, Header, Modal, SortKey};
use crate::services::api::{
    fetch_wallet_holdings, trade_buy, trade_sell, BuyTokenRequest, SellTokenRequest, WalletHolding,
};
use crate::utils::format::truncate;

// Minimal state for the buy dialog: which mint is open + the SOL input value.
#[derive(Clone, PartialEq)]
struct BuyDialog {
    mint: String,
    token_program_id: String,
    sol_input: String,
}

#[function_component(WalletPage)]
pub fn wallet_page() -> Html {
    let holdings = use_state(Vec::<WalletHolding>::new);
    let loading = use_state(|| false);
    let error = use_state(|| Option::<String>::None);
    let action_error = use_state(|| Option::<String>::None);
    let action_success = use_state(|| Option::<String>::None);
    let selling_mint = use_state(|| Option::<String>::None);
    let buy_dialog = use_state(|| Option::<BuyDialog>::None);

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

    // ── Sell all ─────────────────────────────────────────────────────────────
    let on_sell = {
        let selling_mint = selling_mint.clone();
        let action_error = action_error.clone();
        let action_success = action_success.clone();
        let fetch = fetch.clone();
        let holdings = holdings.clone();
        Callback::from(move |(mint, token_amount): (String, u64)| {
            let selling_mint = selling_mint.clone();
            let action_error = action_error.clone();
            let action_success = action_success.clone();
            let fetch = fetch.clone();
            let holdings = holdings.clone();
            selling_mint.set(Some(mint.clone()));
            action_error.set(None);
            action_success.set(None);
            spawn_local(async move {
                let token_account = holdings.iter().find(|h| h.mint == mint).map(|h| h.token_account.clone());
                if let Some(token_account) = token_account {
                    match trade_sell(&SellTokenRequest { mint, token_amount, token_account }).await {
                        Ok(_) => {
                            action_success.set(Some("Sell successful! Refreshing…".into()));
                            TimeoutFuture::new(1_500).await;
                            fetch.emit(());
                        }
                        Err(e) => { action_error.set(Some(format!("Sell failed: {e}"))); }
                    }
                } else {
                    action_error.set(Some("Token account not found for mint".to_string()));
                }
                selling_mint.set(None);
            });
        })
    };

    // ── Buy dialog open ───────────────────────────────────────────────────────
    let on_buy_open = {
        let buy_dialog = buy_dialog.clone();
        Callback::from(move |(mint, token_program_id): (String, String)| {
            buy_dialog.set(Some(BuyDialog {
                mint,
                token_program_id,
                sol_input: "0.1".to_string(),
            }));
        })
    };

    let on_buy_cancel = {
        let buy_dialog = buy_dialog.clone();
        Callback::from(move |_: MouseEvent| buy_dialog.set(None))
    };

    let on_modal_close = {
        let buy_dialog = buy_dialog.clone();
        Callback::from(move |_: ()| buy_dialog.set(None))
    };

    // SOL input change inside dialog
    let on_sol_input = {
        let buy_dialog = buy_dialog.clone();
        Callback::from(move |e: InputEvent| {
            let input = e.target_unchecked_into::<web_sys::HtmlInputElement>().value();
            buy_dialog.set(buy_dialog.as_ref().map(|d| BuyDialog {
                sol_input: input,
                ..d.clone()
            }));
        })
    };

    // Buy submit
    let on_buy_submit = {
        let buy_dialog = buy_dialog.clone();
        let action_error = action_error.clone();
        let action_success = action_success.clone();
        let fetch = fetch.clone();
        Callback::from(move |_: MouseEvent| {
            let Some(dialog) = (*buy_dialog).clone() else { return };
            let sol_amount: f64 = match dialog.sol_input.trim().parse() {
                Ok(v) if v > 0.0 => v,
                _ => {
                    action_error.set(Some("Enter a valid SOL amount > 0".to_string()));
                    return;
                }
            };
            let buy_dialog = buy_dialog.clone();
            let action_error = action_error.clone();
            let action_success = action_success.clone();
            let fetch = fetch.clone();
            action_error.set(None);
            action_success.set(None);
            buy_dialog.set(None);
            spawn_local(async move {
                let req = BuyTokenRequest {
                    mint: dialog.mint,
                    sol_amount,
                    token_program_id: dialog.token_program_id,
                };
                match trade_buy(&req).await {
                    Ok(_) => {
                        action_success.set(Some("Buy successful! Refreshing…".into()));
                        TimeoutFuture::new(1_500).await;
                        fetch.emit(());
                    }
                    Err(e) => { action_error.set(Some(format!("Buy failed: {e}"))); }
                }
            });
        })
    };

    // ── Action column (needs cloned callbacks captured into Rc closure) ───────
    let action_col = {
        let on_sell = on_sell.clone();
        let on_buy_open = on_buy_open.clone();
        let selling_mint = selling_mint.clone();
        Column {
            key: "actions",
            label: "Actions",
            render: Rc::new(move |r: &WalletHolding| {
                let mint = r.mint.clone();
                let token_amount = r.amount;
                let token_program_id = r.token_program_id.clone();
                let is_selling = selling_mint.as_ref().map(|m| m == &mint).unwrap_or(false);

                let sell_cb = {
                    let on_sell = on_sell.clone();
                    let mint = mint.clone();
                    Callback::from(move |_: MouseEvent| on_sell.emit((mint.clone(), token_amount)))
                };
                let buy_cb = {
                    let on_buy_open = on_buy_open.clone();
                    let mint = mint.clone();
                    Callback::from(move |_: MouseEvent| {
                        on_buy_open.emit((mint.clone(), token_program_id.clone()))
                    })
                };

                html! {
                    <div class="action-btns">
                        <button class="btn-action btn-buy" onclick={buy_cb}>{ "Buy" }</button>
                        <button
                            class="btn-action btn-sell"
                            onclick={sell_cb}
                            disabled={is_selling}
                        >
                            { if is_selling { "Selling…" } else { "Sell All" } }
                        </button>
                    </div>
                }
            }),
            sort_value: None,
            search_value: Rc::new(|_: &WalletHolding| String::new()),
            cell_class: None,
            sortable: false,
            default_visible: true,
            width: Some("160px"),
        }
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
        action_col
    ];

    // ── Buy modal values ──────────────────────────────────────────────────────
    let (modal_title, modal_desc, sol_input_val) = if let Some(dialog) = &*buy_dialog {
        let symbol = holdings
            .iter()
            .find(|h| h.mint == dialog.mint)
            .and_then(|h| h.symbol.as_deref())
            .unwrap_or(&dialog.mint);
            // .unwrap_or(&dialog.mint[..8.min(dialog.mint.len())]);
        (
            format!("Buy {}", symbol),
            format!("Mint: {}", dialog.mint),
            // format!("Mint: {}", truncate(&dialog.mint, 16)),
            dialog.sol_input.clone(),
        )
    } else {
        (String::new(), String::new(), String::new())
    };

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
                if let Some(err) = &*action_error {
                    <div class="inline-error">{ err }</div>
                }
                if let Some(msg) = &*action_success {
                    <div class="inline-success">{ msg }</div>
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
            <Modal
                title={modal_title}
                visible={(*buy_dialog).is_some()}
                on_close={on_modal_close}
            >
                <p class="modal-desc">{ modal_desc }</p>
                <label class="modal-label">
                    { "SOL Amount" }
                    <input
                        type="number"
                        class="form-input"
                        min="0.001"
                        step="0.01"
                        value={sol_input_val}
                        oninput={on_sol_input}
                    />
                </label>
                <div class="form-actions">
                    <button class="btn-ghost" onclick={on_buy_cancel}>{ "Cancel" }</button>
                    <button class="btn-primary-sm" onclick={on_buy_submit}>{ "Confirm Buy" }</button>
                </div>
            </Modal>
        </div>
    }
}
