use std::rc::Rc;

use gloo::timers::callback::{Interval, Timeout};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{
    DataTable, FilterPanel, Filters, Header, StatusButton, StatusState,
    TokenDetailPanel, token_columns,
};
use crate::services::api::{fetch_token_detail, fetch_tokens, TokenDetailRecord, POLL_INTERVAL_MS};
use crate::state::{PriceUnitContext, TokenAction, TokenContext};

#[function_component(TokensPage)]
pub fn tokens_page() -> Html {
    let token_state =
        use_context::<TokenContext>().expect("TokenProvider must be mounted above TokensPage");
    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above TokensPage");
    let selected_detail = use_state(|| Option::<TokenDetailRecord>::None);
    let detail_loading = use_state(|| false);
    let detail_error = use_state(|| Option::<String>::None);
    let tick = use_state(|| 0u32);
    let tick_ref = use_mut_ref(|| 0u32);
    let live = use_state(load_live);
    let show_filters = use_state(|| false);
    let filters = use_state(Filters::default);

    // ── Polling interval ──────────────────────────────────────────────────────
    {
        let tick = tick.clone();
        let tick_ref = tick_ref.clone();
        use_effect_with((), move |_| {
            let interval = Interval::new(POLL_INTERVAL_MS, move || {
                let mut v = tick_ref.borrow_mut();
                *v = v.wrapping_add(1);
                tick.set(*v);
            });
            move || drop(interval)
        });
    }

    // ── Persist live toggle ───────────────────────────────────────────────────
    {
        let live_val = *live;
        use_effect_with(live_val, move |_| { save_live(live_val); || () });
    }

    // ── Reset to page 1 when filters change ──────────────────────────────────
    // (DataTable handles pagination internally; nothing needed here)

    // ── Fetch ALL tokens on mount ─────────────────────────────────────────────
    {
        let token_state = token_state.clone();
        use_effect_with((), move |_| {
            token_state.dispatch(TokenAction::SetLoading);
            spawn_local(async move {
                match fetch_tokens("", 5000, 0).await {
                    Ok(result) => token_state.dispatch(TokenAction::SetTokens { tokens: result.items, total: result.total }),
                    Err(e) => token_state.dispatch(TokenAction::SetError(e)),
                }
            });
            || ()
        });
    }

    // ── Periodic silent refresh ───────────────────────────────────────────────
    {
        let token_state = token_state.clone();
        let tick_val = *tick;
        let live_val = *live;
        use_effect_with((tick_val, live_val), move |(tick_val, live_val)| {
            let cleanup = || ();
            if *tick_val == 0 || !*live_val { return cleanup; }
            spawn_local(async move {
                match fetch_tokens("", 5000, 0).await {
                    Ok(result) => token_state.dispatch(TokenAction::SetTokens { tokens: result.items, total: result.total }),
                    Err(e) => token_state.dispatch(TokenAction::SetError(e)),
                }
            });
            cleanup
        });
    }

    // ── Fetch detail when selection changes ───────────────────────────────────
    {
        let selected_mint = token_state.selected_mint.clone();
        let selected_detail = selected_detail.clone();
        let detail_loading = detail_loading.clone();
        let detail_error = detail_error.clone();
        use_effect_with(selected_mint.clone(), move |mint| {
            if let Some(mint) = mint.as_ref() {
                let mint = mint.clone();
                detail_loading.set(true);
                detail_error.set(None);
                selected_detail.set(None);
                spawn_local(async move {
                    match fetch_token_detail(&mint).await {
                        Ok(detail) => selected_detail.set(Some(detail)),
                        Err(err) => detail_error.set(Some(err)),
                    }
                    detail_loading.set(false);
                });
            } else {
                selected_detail.set(None);
                detail_error.set(None);
                detail_loading.set(false);
            }
            || ()
        });
    }

    // ── Scroll detail panel into view after selection ─────────────────────────
    {
        let selected_mint = token_state.selected_mint.clone();
        use_effect_with(selected_mint.clone(), move |mint| {
            if let Some(mint) = mint.as_ref() {
                let mint = mint.clone();
                let handle = Timeout::new(300, move || {
                    if let Some(window) = web_sys::window() {
                        if let Some(doc) = window.document() {
                            let id = format!("detail-{}", mint);
                            if let Some(el) = doc.get_element_by_id(&id) {
                                let opts = js_sys::Object::new();
                                let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("behavior"), &JsValue::from_str("smooth"));
                                let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("block"), &JsValue::from_str("nearest"));
                                let el_val: JsValue = el.clone().into();
                                if let Ok(f) = js_sys::Reflect::get(&el_val, &JsValue::from_str("scrollIntoView")) {
                                    if let Ok(func) = f.dyn_into::<js_sys::Function>() {
                                        let _ = func.call1(&el_val, &opts.into());
                                    }
                                }
                            }
                        }
                    }
                });
                std::mem::forget(handle);
            }
            || ()
        });
    }

    // ── Handlers ─────────────────────────────────────────────────────────────
    let on_toggle_live = {
        let live = live.clone();
        Callback::from(move |_| live.set(!*live))
    };
    let on_toggle_filters = {
        let show_filters = show_filters.clone();
        Callback::from(move |_: MouseEvent| show_filters.set(!*show_filters))
    };
    let on_clear_filters = {
        let filters = filters.clone();
        Callback::from(move |_: MouseEvent| filters.set(Filters::default()))
    };
    let on_filter_change = {
        let filters = filters.clone();
        Callback::from(move |(field, val): (String, String)| {
            let mut f = (*filters).clone();
            match field.as_str() {
                "age_min" => f.age_min = val, "age_max" => f.age_max = val,
                "last_trade_min" => f.last_trade_min = val, "last_trade_max" => f.last_trade_max = val,
                "ath_age_min" => f.ath_age_min = val, "ath_age_max" => f.ath_age_max = val,
                "ath_fep_min" => f.ath_fep_min = val, "ath_fep_max" => f.ath_fep_max = val,
                "cur_fep_min" => f.cur_fep_min = val, "cur_fep_max" => f.cur_fep_max = val,
                "ath_price_min" => f.ath_price_min = val, "ath_price_max" => f.ath_price_max = val,
                "price_min" => f.price_min = val, "price_max" => f.price_max = val,
                "volume_min" => f.volume_min = val, "volume_max" => f.volume_max = val,
                "mcap_min" => f.mcap_min = val, "mcap_max" => f.mcap_max = val,
                "init_buy_min" => f.init_buy_min = val, "init_buy_max" => f.init_buy_max = val,
                "init_supply_min" => f.init_supply_min = val, "init_supply_max" => f.init_supply_max = val,
                "token_amount_min" => f.token_amount_min = val, "token_amount_max" => f.token_amount_max = val,
                "max_sol_cost_min" => f.max_sol_cost_min = val, "max_sol_cost_max" => f.max_sol_cost_max = val,
                "spendable_sol_in_min" => f.spendable_sol_in_min = val, "spendable_sol_in_max" => f.spendable_sol_in_max = val,
                "min_tokens_out_min" => f.min_tokens_out_min = val, "min_tokens_out_max" => f.min_tokens_out_max = val,
                "trades_min" => f.trades_min = val, "trades_max" => f.trades_max = val,
                "cu_limit_min" => f.cu_limit_min = val, "cu_limit_max" => f.cu_limit_max = val,
                "cu_price_min" => f.cu_price_min = val, "cu_price_max" => f.cu_price_max = val,
                "ix_count_min" => f.ix_count_min = val, "ix_count_max" => f.ix_count_max = val,
                "ix_label" => f.ix_label = val, "creator" => f.creator = val,
                _ => {}
            }
            filters.set(f);
        })
    };
    let on_filter_select_change = {
        let filters = filters.clone();
        Callback::from(move |(field, val): (String, String)| {
            let mut f = (*filters).clone();
            match field.as_str() { "migrated" => f.migrated = val, _ => {} }
            filters.set(f);
        })
    };

    let active_filter_count = filters.active_count();
    let filters_active = !filters.is_empty();

    // ── Apply FilterPanel filters (search/sort/pagination handled by DataTable) ─
    let mut displayed_tokens = token_state.tokens.clone();
    if filters_active {
        displayed_tokens.retain(|t| filters.passes(t));
    }

    // ── Column definitions ────────────────────────────────────────────────────
    let columns = token_columns(price_unit);

    // ── Detail panel closure ──────────────────────────────────────────────────
    use crate::services::api::TokenRecord;
    let row_detail: Rc<dyn Fn(&TokenRecord) -> Html> = {
        let selected_detail = selected_detail.clone();
        let detail_loading = detail_loading.clone();
        let detail_error = detail_error.clone();
        Rc::new(move |_r: &TokenRecord| {
            html! {
                <TokenDetailPanel
                    detail={(*selected_detail).clone()}
                    loading={*detail_loading}
                    error={(*detail_error).clone()}
                />
            }
        })
    };
    let row_key: Rc<dyn Fn(&TokenRecord) -> String> =
        Rc::new(|r: &TokenRecord| r.mint_address.clone());

    // ── Selection callback ────────────────────────────────────────────────────
    let on_select = {
        let token_state = token_state.clone();
        Callback::from(move |key: Option<String>| {
            match key {
                Some(mint) => token_state.dispatch(TokenAction::SelectToken(mint)),
                None => token_state.dispatch(TokenAction::ClearSelection),
            }
        })
    };

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="tokens-page-header">
                    <div class="tokens-title-row">
                        <h2 class="tokens-page-title">{ "Tokens" }</h2>
                        <span class="token-count-badge">{ format!("{} tracked", token_state.total) }</span>
                        <StatusButton
                            state={if *live { StatusState::Live } else { StatusState::Dead }}
                            onclick={on_toggle_live}
                            class={"live-toggle-btn".to_string()}
                            label={Some(if *live { "ACTIVE" } else { "PAUSED" }.to_string())}
                        />
                    </div>
                </div>

                <div class="tokens-options-bar">
                    <button
                        class={classes!("options-toggle-btn", (active_filter_count > 0 || *show_filters).then_some("options-toggle-active"))}
                        onclick={on_toggle_filters}
                    >
                        { if active_filter_count > 0 { format!("Filters ({})", active_filter_count) } else { "Filters".to_string() } }
                    </button>
                </div>

                if *show_filters {
                    <FilterPanel
                        filters={(*filters).clone()}
                        active_filter_count={active_filter_count}
                        on_clear={on_clear_filters}
                        on_change={on_filter_change}
                        on_select_change={on_filter_select_change}
                    />
                }

                if token_state.loading {
                    <p class="loading">{ "Loading tokens…" }</p>
                } else if let Some(err) = &token_state.error {
                    <p class="error">{ err }</p>
                } else {
                    <DataTable<TokenRecord>
                        columns={columns}
                        rows={displayed_tokens}
                        row_key={row_key}
                        row_detail={Some(row_detail)}
                        on_select={Some(on_select)}
                        selected_key={token_state.selected_mint.clone()}
                        default_page_size={25}
                        page_size_options={vec![10usize, 25, 50, 100]}
                        searchable={true}
                        col_filters={true}
                        col_toggle={true}
                        hoverable={true}
                        storage_key={"tokens_visible_cols"}
                        item_label="tokens"
                        empty_message="No tokens found"
                    />
                }
            </main>
        </div>
    }
}

// ── localStorage helpers ──────────────────────────────────────────────────────

const LS_LIVE_KEY: &str = "tokens_live";

fn load_live() -> bool {
    let window = match web_sys::window() { Some(w) => w, None => return false };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return false };
    matches!(storage.get_item(LS_LIVE_KEY).ok().flatten().as_deref(), Some("true"))
}

fn save_live(live: bool) {
    let window = match web_sys::window() { Some(w) => w, None => return };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return };
    let _ = storage.set_item(LS_LIVE_KEY, if live { "true" } else { "false" });
}
