use gloo::timers::callback::Interval;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{
    compute_group_boundaries, ColOptionsPanel, FilterPanel, Filters, Header, Pagination,
    StatusButton, StatusState, TokensTable, COLUMNS,
};
use crate::services::api::{fetch_token_detail, fetch_tokens, TokenDetailRecord, POLL_INTERVAL_MS};
use crate::state::{sort_tokens, TokenAction, TokenContext};
use gloo::timers::callback::Timeout;
use js_sys;
use std::collections::HashSet;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys;

const PAGE_SIZE_OPTIONS: &[usize] = &[10, 25, 50, 100];

const LS_COL_KEY: &str = "tokens_visible_cols";
const LS_LIVE_KEY: &str = "tokens_live";

fn load_live() -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return false,
    };
    match storage.get_item(LS_LIVE_KEY).ok().flatten().as_deref() {
        Some("true") => true,
        _ => false,
    }
}

fn save_live(live: bool) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return,
    };
    let _ = storage.set_item(LS_LIVE_KEY, if live { "true" } else { "false" });
}

fn default_visible_cols() -> HashSet<String> {
    COLUMNS.iter().map(|(k, _, _, _)| k.to_string()).collect()
}

fn load_visible_cols() -> HashSet<String> {
    let default = default_visible_cols();
    let window = match web_sys::window() {
        Some(w) => w,
        None => return default,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return default,
    };
    let raw = match storage.get_item(LS_COL_KEY).ok().flatten() {
        Some(v) => v,
        None => return default,
    };
    match js_sys::JSON::parse(&raw) {
        Ok(obj) => {
            let arr = js_sys::Array::from(&obj);
            let parsed: HashSet<String> = arr
                .iter()
                .filter_map(|v| v.as_string())
                .filter(|s| COLUMNS.iter().any(|(k, _, _, _)| *k == s.as_str()))
                .collect();
            if parsed.is_empty() {
                default
            } else {
                parsed
            }
        }
        Err(_) => default,
    }
}

fn save_visible_cols(cols: &HashSet<String>) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return,
    };
    let arr = js_sys::Array::new();
    for key in cols.iter() {
        arr.push(&JsValue::from_str(key));
    }
    if let Ok(json) = js_sys::JSON::stringify(&arr) {
        if let Some(s) = json.as_string() {
            let _ = storage.set_item(LS_COL_KEY, &s);
        }
    }
}

// ── Page ──────────────────────────────────────────────────────────────────────

#[function_component(TokensPage)]
pub fn tokens_page() -> Html {
    let token_state =
        use_context::<TokenContext>().expect("TokenProvider must be mounted above TokensPage");
    let search = use_state(String::new);
    let page = use_state(|| 1usize);
    let page_size = use_state(|| 25usize);
    // Tokens, total, loading and error are now kept in the shared `TokenContext`.
    let selected_detail = use_state(|| Option::<TokenDetailRecord>::None);
    let detail_loading = use_state(|| false);
    let detail_error = use_state(|| Option::<String>::None);
    let tick = use_state(|| 0u32);
    let tick_ref = use_mut_ref(|| 0u32);
    let live = use_state(load_live);
    let show_col_opts = use_state(|| false);
    let visible_cols = use_state(load_visible_cols);
    let show_filters = use_state(|| false);
    let filters = use_state(Filters::default);
    let hovered_col = use_state(|| None::<usize>);
    // Full token list is always in token_state.tokens (fetched all at once).

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

    // (SSE connection for token deltas removed; StatusButton now only controls polling)

    // ── Persist column visibility to localStorage ──────────────────────────────────
    {
        let visible_cols = visible_cols.clone();
        use_effect_with((*visible_cols).clone(), move |cols| {
            save_visible_cols(cols);
            || ()
        });
    }
    // ── Persist live toggle to localStorage ───────────────────────────────────
    {
        let live_val = *live;
        use_effect_with(live_val, move |&val| {
            save_live(val);
            || ()
        });
    }
    // ── Reset to page 1 when filters change ────────────────────────────────────
    {
        let page = page.clone();
        use_effect_with((*filters).clone(), move |_| {
            page.set(1);
            || ()
        });
    }

    // ── Reset to page 1 when search changes ──────────────────────────────────
    {
        let page = page.clone();
        use_effect_with((*search).clone(), move |_| {
            page.set(1);
            || ()
        });
    }

    // ── Keep page state valid when total or page size changes ─────────────────
    {
        let page = page.clone();
        let page_size = *page_size;
        let total = token_state.total;
        use_effect_with(
            (total, page_size, *page),
            move |(total, page_size, current_page)| {
                let total_pages = if *total == 0 {
                    1
                } else {
                    (*total + page_size - 1) / page_size
                };
                if *current_page > total_pages {
                    page.set(total_pages);
                }
                || ()
            },
        );
    }

    // ── Fetch ALL tokens on mount (client-side pagination/search/filters after) ──
    {
        let token_state = token_state.clone();
        use_effect_with((), move |_| {
            token_state.dispatch(TokenAction::SetLoading);
            spawn_local(async move {
                match fetch_tokens("", 5000, 0).await {
                    Ok(result) => {
                        token_state.dispatch(TokenAction::SetTokens {
                            tokens: result.items,
                            total: result.total,
                        });
                    }
                    Err(e) => {
                        token_state.dispatch(TokenAction::SetError(e));
                    }
                }
            });
            || ()
        });
    }

    // ── Periodic refresh — re-fetches all tokens silently ──────────────────────
    {
        let token_state = token_state.clone();
        let tick_val = *tick;
        let live_val = *live;
        use_effect_with((tick_val, live_val), move |(tick_val, live_val)| {
            let cleanup = || ();
            if *tick_val == 0 || !*live_val {
                return cleanup;
            }
            spawn_local(async move {
                match fetch_tokens("", 5000, 0).await {
                    Ok(result) => {
                        token_state.dispatch(TokenAction::SetTokens {
                            tokens: result.items,
                            total: result.total,
                        });
                    }
                    Err(e) => {
                        token_state.dispatch(TokenAction::SetError(e));
                    }
                }
            });
            cleanup
        });
    }

    // ── Fetch detail when selection changes ──────────────────────────────────────
    {
        let selected_mint = token_state.selected_mint.clone();
        let selected_detail = selected_detail.clone();
        let detail_loading = detail_loading.clone();
        let detail_error = detail_error.clone();
        use_effect_with(selected_mint.clone(), move |mint| {
            let mint = mint.clone();
            if let Some(mint) = mint {
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

    // ── Scroll detail panel into view after selection ───────────────────────
    {
        let selected_mint = token_state.selected_mint.clone();
        use_effect_with(selected_mint, move |mint| {
            if let Some(mint) = mint.as_ref() {
                let mint = mint.clone();
                let handle = Timeout::new(300, move || {
                    if let Some(window) = web_sys::window() {
                        if let Some(doc) = window.document() {
                            let id = format!("detail-{}", mint);
                            if let Some(el) = doc.get_element_by_id(&id) {
                                let opts = js_sys::Object::new();
                                let _ = js_sys::Reflect::set(
                                    &opts,
                                    &JsValue::from_str("behavior"),
                                    &JsValue::from_str("smooth"),
                                );
                                let _ = js_sys::Reflect::set(
                                    &opts,
                                    &JsValue::from_str("block"),
                                    &JsValue::from_str("nearest"),
                                );
                                let el_val: JsValue = el.clone().into();
                                if let Ok(f) = js_sys::Reflect::get(
                                    &el_val,
                                    &JsValue::from_str("scrollIntoView"),
                                ) {
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
    let on_search = {
        let search = search.clone();
        Callback::from(move |e: InputEvent| {
            let el: web_sys::HtmlInputElement = e.target_unchecked_into();
            search.set(el.value());
        })
    };

    let on_page_change = {
        let page = page.clone();
        Callback::from(move |new_page: usize| {
            page.set(new_page);
        })
    };

    let on_page_size_change = {
        let page_size = page_size.clone();
        let page = page.clone();
        Callback::from(move |size: usize| {
            page_size.set(size);
            page.set(1);
        })
    };

    // `on_select_token` is defined later after auto-scroll state so it can
    // set the `force_scroll` flag when a user clicks a row.

    let on_toggle_sort = {
        let token_state = token_state.clone();
        Callback::from(move |field: String| {
            token_state.dispatch(TokenAction::ToggleSort(field));
        })
    };

    let on_toggle_live = {
        let live = live.clone();
        Callback::from(move |_| {
            live.set(!*live);
        })
    };

    let on_toggle_col_opts = {
        let show_col_opts = show_col_opts.clone();
        Callback::from(move |_: MouseEvent| show_col_opts.set(!*show_col_opts))
    };

    let on_toggle_col = {
        let visible_cols = visible_cols.clone();
        Callback::from(move |key: String| {
            let mut new_cols = (*visible_cols).clone();
            if new_cols.contains(&key) {
                if new_cols.len() > 1 {
                    new_cols.remove(&key);
                }
            } else {
                new_cols.insert(key);
            }
            visible_cols.set(new_cols);
        })
    };

    let on_toggle_filters = {
        let show_filters = show_filters.clone();
        Callback::from(move |_: MouseEvent| show_filters.set(!*show_filters))
    };

    let on_clear_filters = {
        let filters = filters.clone();
        Callback::from(move |_: MouseEvent| {
            filters.set(Filters::default());
        })
    };

    let on_filter_change = {
        let filters = filters.clone();
        Callback::from(move |(field, val): (String, String)| {
            let mut f = (*filters).clone();
            match field.as_str() {
                "age_min" => f.age_min = val,
                "age_max" => f.age_max = val,
                "last_trade_min" => f.last_trade_min = val,
                "last_trade_max" => f.last_trade_max = val,
                "ath_age_min" => f.ath_age_min = val,
                "ath_age_max" => f.ath_age_max = val,
                "ath_fep_min" => f.ath_fep_min = val,
                "ath_fep_max" => f.ath_fep_max = val,
                "cur_fep_min" => f.cur_fep_min = val,
                "cur_fep_max" => f.cur_fep_max = val,
                "ath_price_min" => f.ath_price_min = val,
                "ath_price_max" => f.ath_price_max = val,
                "price_min" => f.price_min = val,
                "price_max" => f.price_max = val,
                "volume_min" => f.volume_min = val,
                "volume_max" => f.volume_max = val,
                "mcap_min" => f.mcap_min = val,
                "mcap_max" => f.mcap_max = val,
                "init_buy_min" => f.init_buy_min = val,
                "init_buy_max" => f.init_buy_max = val,
                "init_supply_min" => f.init_supply_min = val,
                "init_supply_max" => f.init_supply_max = val,
                "token_amount_min" => f.token_amount_min = val,
                "token_amount_max" => f.token_amount_max = val,
                "max_sol_cost_min" => f.max_sol_cost_min = val,
                "max_sol_cost_max" => f.max_sol_cost_max = val,
                "spendable_sol_in_min" => f.spendable_sol_in_min = val,
                "spendable_sol_in_max" => f.spendable_sol_in_max = val,
                "min_tokens_out_min" => f.min_tokens_out_min = val,
                "min_tokens_out_max" => f.min_tokens_out_max = val,
                "trades_min" => f.trades_min = val,
                "trades_max" => f.trades_max = val,
                "cu_limit_min" => f.cu_limit_min = val,
                "cu_limit_max" => f.cu_limit_max = val,
                "cu_price_min" => f.cu_price_min = val,
                "cu_price_max" => f.cu_price_max = val,
                "ix_count_min" => f.ix_count_min = val,
                "ix_count_max" => f.ix_count_max = val,
                "ix_label" => f.ix_label = val,
                "creator" => f.creator = val,
                _ => {}
            }
            filters.set(f);
        })
    };

    let on_filter_select_change = {
        let filters = filters.clone();
        Callback::from(move |(field, val): (String, String)| {
            let mut f = (*filters).clone();
            match field.as_str() {
                "migrated" => f.migrated = val,
                _ => {}
            }
            filters.set(f);
        })
    };

    let active_filter_count = filters.active_count();

    // ── Pagination + data preparation ─────────────────────────────────────────────
    let total = token_state.total;
    let ps = *page_size;
    let filters_active = !filters.is_empty();

    // All tokens are in state — always sort, search, filter and paginate client-side.
    let mut displayed_tokens = token_state.tokens.clone();
    sort_tokens(&mut displayed_tokens, &token_state.sort);

    // Client-side search
    if !(*search).is_empty() {
        let needle = (*search).to_lowercase();
        displayed_tokens.retain(|t| {
            t.mint_address.to_lowercase().contains(&needle)
                || t.symbol.to_lowercase().contains(&needle)
                || t.name.to_lowercase().contains(&needle)
        });
    }

    // Client-side filters
    if filters_active {
        displayed_tokens.retain(|t| filters.passes(t));
    }

    // Effective total: always client-side after search+filter.
    let effective_total = displayed_tokens.len();
    let total_pages = if effective_total == 0 {
        1
    } else {
        (effective_total + ps - 1) / ps
    };
    let cur_page = (*page).clamp(1, total_pages);
    let offset = cur_page.saturating_sub(1) * ps;

    // Slice to current page.
    displayed_tokens = displayed_tokens.into_iter().skip(offset).take(ps).collect();

    // Build visibility mask (one bool per COLUMNS entry, in order)
    let vis: Vec<bool> = COLUMNS
        .iter()
        .map(|(key, _, _, _)| (*visible_cols).contains(*key))
        .collect();
    let group_border_cols = compute_group_boundaries(&vis);
    // total rendered columns: # + visible cols + action
    let num_cols = 2 + vis.iter().filter(|&&b| b).count();

    let on_col_hover = {
        let hovered_col = hovered_col.clone();
        Callback::from(move |col: Option<usize>| hovered_col.set(col))
    };

    let on_select_token = {
        let token_state = token_state.clone();
        use_callback(token_state, move |mint: String, token_state| {
            token_state.dispatch(TokenAction::SelectToken(mint));
        })
    };

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                // ── Page header ─────────────────────────────────────────────────
                <div class="tokens-page-header">
                    <div class="tokens-title-row">
                        <h2 class="tokens-page-title">{ "Tokens" }</h2>
                        <span class="token-count-badge">{ format!("{} tracked", total) }</span>
                        <StatusButton
                            state={if *live { StatusState::Live } else { StatusState::Dead }}
                            onclick={on_toggle_live}
                            class={"live-toggle-btn".to_string()}
                            label={Some(if *live { "ACTIVE" } else { "PAUSED" }.to_string())}
                        />
                    </div>
                </div>

                // ── Toolbar: search + pagination ─────────────────────────────────
                <div class="tokens-toolbar">
                    <input
                        type="text"
                        class="tokens-search"
                        placeholder="Search by mint or symbol..."
                        value={(*search).clone()}
                        oninput={on_search}
                    />
                    <Pagination
                        current_page={cur_page}
                        total_pages={total_pages}
                        total_items={effective_total}
                        page_size={ps}
                        page_size_options={PAGE_SIZE_OPTIONS.to_vec()}
                        on_page_change={on_page_change}
                        on_page_size_change={on_page_size_change}
                    />
                </div>

                // ── Column options toggle + panel ──────────────────────────────
                <div class="tokens-options-bar">
                    <button
                        class={classes!("options-toggle-btn", (*show_col_opts).then_some("options-toggle-active"))}
                        onclick={on_toggle_col_opts}
                    >
                        { if *show_col_opts { "Hide Column Options" } else { "Show Column Options" } }
                    </button>
                    <button
                        class={classes!("options-toggle-btn", (active_filter_count > 0 || *show_filters).then_some("options-toggle-active"))}
                        onclick={on_toggle_filters}
                    >
                        { if active_filter_count > 0 {
                            format!("Filters ({})", active_filter_count)
                        } else {
                            "Filters".to_string()
                        } }
                    </button>
                </div>

                if *show_col_opts {
                    <ColOptionsPanel
                        visible_cols={(*visible_cols).clone()}
                        on_toggle_col={on_toggle_col.clone()}
                    />
                }

                if *show_filters {
                    <FilterPanel
                        filters={(*filters).clone()}
                        active_filter_count={active_filter_count}
                        on_clear={on_clear_filters.clone()}
                        on_change={on_filter_change}
                        on_select_change={on_filter_select_change}
                    />
                }

                if token_state.loading {
                    <p class="loading">{ "Loading tokens…" }</p>
                } else if let Some(err) = &token_state.error {
                    <p class="error">{ err }</p>
                } else {
                    <TokensTable
                        tokens={displayed_tokens.clone()}
                        visible_cols={vis.clone()}
                        group_borders={group_border_cols.clone()}
                        num_cols={num_cols}
                        sort={token_state.sort.clone()}
                        on_toggle_sort={on_toggle_sort}
                        on_select_token={on_select_token.clone()}
                        hovered_column={*hovered_col}
                        on_hover_column={on_col_hover.clone()}
                        selected_mint={token_state.selected_mint.clone()}
                        selected_detail={(*selected_detail).clone()}
                        detail_loading={*detail_loading}
                        detail_error={(*detail_error).clone()}
                        offset={offset}
                    />
                }
            </main>
        </div>
    }
}
