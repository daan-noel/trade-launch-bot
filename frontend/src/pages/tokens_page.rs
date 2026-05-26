use gloo::timers::callback::Interval;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{Header, Pagination, TokenRow};
use crate::services::api::{
    fetch_token_detail, fetch_tokens, TokenDetailRecord, TokenRecord, POLL_INTERVAL_MS,
};
use web_sys;
use js_sys;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use gloo::timers::callback::Timeout;
use crate::services::websocket::connect_sse_tokens;
use crate::state::{sort_tokens, SortOrder, TokenAction, TokenContext};
use std::collections::HashSet;

const PAGE_SIZE_OPTIONS: &[usize] = &[10, 25, 50, 100];

/// (sort_key, display_label, col_width_px, optional_th_class)
const COLUMNS: &[(&str, &str, u32, Option<&str>)] = &[
    // Identity
    ("symbol",            "Symbol",       90,  None),
    ("name",              "Name",         120, None),
    ("mint",              "Mint",         130, None),
    ("creator",           "Creator",      130, None),
    // Lifecycle
    ("age",               "Age",           72, None),
    ("created",           "Created",      110, None),
    ("last_trade",        "Last Trade",   110, None),
    ("migrated",          "Migrated",      66, None),
    ("mayhem_mode",       "Mayhem",        66, None),
    // Performance
    ("ath_fep_ratio",     "ATH/FEP",       88, Some("th-ath-fep")),
    ("current_fep_ratio", "Cur/FEP",       76, Some("th-cur-fep")),
    ("ath_price",         "ATH",           88, None),
    ("ath_timestamp",     "ATH At",       110, None),
    ("current_price",     "Price",         88, None),
    // Market
    ("market_cap",        "MCap",          84, None),
    ("volume",            "Volume",        78, None),
    ("initial_buy",       "Init Buy",      78, None),
    ("init_supply",       "Init Supply",   90, None),
    ("trade_count",       "Trades",        66, None),
    // Technical
    ("cu_limit",          "CU Limit",      72, None),
    ("cu_price",          "CU Price",      72, None),
    ("ix_count",          "IX Count",      54, None),
    ("ix_labels",         "IX Labels",    180, None),
    ("create_tx",         "Create TX",    130, None),
];

const LS_COL_KEY: &str  = "tokens_visible_cols";
const LS_LIVE_KEY: &str = "tokens_live";

fn load_live() -> bool {
    let window = match web_sys::window() { Some(w) => w, None => return true };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return true };
    match storage.get_item(LS_LIVE_KEY).ok().flatten().as_deref() {
        Some("false") => false,
        _             => true,
    }
}

fn save_live(live: bool) {
    let window = match web_sys::window() { Some(w) => w, None => return };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return };
    let _ = storage.set_item(LS_LIVE_KEY, if live { "true" } else { "false" });
}

fn default_visible_cols() -> HashSet<String> {
    COLUMNS.iter().map(|(k, _, _, _)| k.to_string()).collect()
}

fn load_visible_cols() -> HashSet<String> {
    let default = default_visible_cols();
    let window = match web_sys::window() { Some(w) => w, None => return default };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return default };
    let raw = match storage.get_item(LS_COL_KEY).ok().flatten() { Some(v) => v, None => return default };
    match js_sys::JSON::parse(&raw) {
        Ok(obj) => {
            let arr = js_sys::Array::from(&obj);
            let parsed: HashSet<String> = arr.iter()
                .filter_map(|v| v.as_string())
                .filter(|s| COLUMNS.iter().any(|(k, _, _, _)| *k == s.as_str()))
                .collect();
            if parsed.is_empty() { default } else { parsed }
        }
        Err(_) => default,
    }
}

fn save_visible_cols(cols: &HashSet<String>) {
    let window = match web_sys::window() { Some(w) => w, None => return };
    let storage = match window.local_storage().ok().flatten() { Some(s) => s, None => return };
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

// ── Filters ───────────────────────────────────────────────────────────────────

/// Parse an ISO-8601 datetime string and return how many hours ago it was.
/// Uses `js_sys::Date::parse` which is always available in the browser/WASM.
fn iso_hours_ago(s: &str) -> Option<f64> {
    let ts_ms = js_sys::Date::parse(s);
    if ts_ms.is_nan() {
        return None;
    }
    let now_ms = js_sys::Date::now();
    Some((now_ms - ts_ms).max(0.0) / 3_600_000.0)
}

#[derive(Clone, PartialEq, Default)]
struct Filters {
    // ── Time ──────────────────────────────────────────────────────────────────
    age_min:          String,  // hours since creation
    age_max:          String,
    last_trade_min:   String,  // hours since last trade
    last_trade_max:   String,
    ath_age_min:      String,  // hours since ATH was set
    ath_age_max:      String,
    // ── Performance ───────────────────────────────────────────────────────────
    ath_fep_min:      String,  // ATH / first-entry-price (×)
    ath_fep_max:      String,
    cur_fep_min:      String,  // Current / first-entry-price (×)
    cur_fep_max:      String,
    ath_price_min:    String,
    ath_price_max:    String,
    price_min:        String,
    price_max:        String,
    // ── Market ────────────────────────────────────────────────────────────────
    volume_min:       String,  // SOL
    volume_max:       String,
    mcap_min:         String,
    mcap_max:         String,
    init_buy_min:     String,
    init_buy_max:     String,
    init_supply_min:  String,  // token units
    init_supply_max:  String,
    trades_min:       String,
    trades_max:       String,
    // ── Technical ─────────────────────────────────────────────────────────────
    cu_limit_min:     String,
    cu_limit_max:     String,
    cu_price_min:     String,
    cu_price_max:     String,
    ix_count_min:     String,  // number of distinct IX labels
    ix_count_max:     String,
    ix_label:         String,  // substring match
    // ── Other ─────────────────────────────────────────────────────────────────
    migrated:         String,  // "" | "yes" | "no"
    creator:          String,  // substring match on creator_address
}

impl Filters {
    fn is_empty(&self) -> bool {
        self.age_min.is_empty()         && self.age_max.is_empty()
        && self.last_trade_min.is_empty() && self.last_trade_max.is_empty()
        && self.ath_age_min.is_empty()  && self.ath_age_max.is_empty()
        && self.ath_fep_min.is_empty()  && self.ath_fep_max.is_empty()
        && self.cur_fep_min.is_empty()  && self.cur_fep_max.is_empty()
        && self.ath_price_min.is_empty() && self.ath_price_max.is_empty()
        && self.price_min.is_empty()    && self.price_max.is_empty()
        && self.volume_min.is_empty()   && self.volume_max.is_empty()
        && self.mcap_min.is_empty()     && self.mcap_max.is_empty()
        && self.init_buy_min.is_empty() && self.init_buy_max.is_empty()
        && self.init_supply_min.is_empty() && self.init_supply_max.is_empty()
        && self.trades_min.is_empty()   && self.trades_max.is_empty()
        && self.cu_limit_min.is_empty() && self.cu_limit_max.is_empty()
        && self.cu_price_min.is_empty() && self.cu_price_max.is_empty()
        && self.ix_count_min.is_empty() && self.ix_count_max.is_empty()
        && self.migrated.is_empty()
        && self.ix_label.is_empty()
        && self.creator.is_empty()
    }

    fn passes(&self, t: &TokenRecord) -> bool {
        // Apply a range to an always-present f64 value.
        macro_rules! range_f64 {
            ($val:expr, $min:expr, $max:expr) => {{
                let val: f64 = $val;
                if let Ok(v) = $min.parse::<f64>() { if val < v { return false; } }
                if let Ok(v) = $max.parse::<f64>() { if val > v { return false; } }
            }};
        }
        // Apply a range to an Option<f64>. If either bound is set and the
        // value is None the token is excluded. Only the filled-in bound applies.
        macro_rules! opt_f64 {
            ($opt:expr, $min:expr, $max:expr) => {{
                if !$min.is_empty() || !$max.is_empty() {
                    match { let x: Option<f64> = $opt; x } {
                        Some(val) => {
                            if let Ok(v) = $min.parse::<f64>() { if val < v { return false; } }
                            if let Ok(v) = $max.parse::<f64>() { if val > v { return false; } }
                        }
                        None => return false,
                    }
                }
            }};
        }

        // ── Time ──────────────────────────────────────────────────────────────
        range_f64!(t.age as f64 / 3600.0, &self.age_min, &self.age_max);

        if !self.last_trade_min.is_empty() || !self.last_trade_max.is_empty() {
            match t.last_trade_at.as_deref().and_then(iso_hours_ago) {
                Some(h) => {
                    if let Ok(v) = self.last_trade_min.parse::<f64>() { if h < v { return false; } }
                    if let Ok(v) = self.last_trade_max.parse::<f64>() { if h > v { return false; } }
                }
                None => return false,
            }
        }

        if !self.ath_age_min.is_empty() || !self.ath_age_max.is_empty() {
            match t.ath_timestamp.as_deref().and_then(iso_hours_ago) {
                Some(h) => {
                    if let Ok(v) = self.ath_age_min.parse::<f64>() { if h < v { return false; } }
                    if let Ok(v) = self.ath_age_max.parse::<f64>() { if h > v { return false; } }
                }
                None => return false,
            }
        }

        // ── Performance ───────────────────────────────────────────────────────
        let fep: Option<f64> = t.initial_buy_sol.and_then(|buy| {
            t.initial_supply_token.and_then(|sup| {
                if sup > 0 { Some(buy / sup as f64) } else { None }
            })
        });
        let ath_fep: Option<f64> = fep.and_then(|p| {
            t.ath_price.and_then(|a| if p > 0.0 { Some(a / p) } else { None })
        });
        let cur_fep: Option<f64> = fep.and_then(|p| {
            t.current_price.and_then(|c| if p > 0.0 { Some(c / p) } else { None })
        });

        opt_f64!(ath_fep,       &self.ath_fep_min,   &self.ath_fep_max);
        opt_f64!(cur_fep,       &self.cur_fep_min,   &self.cur_fep_max);
        opt_f64!(t.ath_price,   &self.ath_price_min, &self.ath_price_max);
        opt_f64!(t.current_price, &self.price_min,   &self.price_max);

        // ── Market ────────────────────────────────────────────────────────────
        range_f64!(t.volume_sol_total, &self.volume_min, &self.volume_max);
        opt_f64!(t.market_cap,     &self.mcap_min,        &self.mcap_max);
        opt_f64!(t.initial_buy_sol, &self.init_buy_min,   &self.init_buy_max);
        opt_f64!(t.initial_supply_token.map(|v| v as f64),
                 &self.init_supply_min, &self.init_supply_max);
        range_f64!(t.trade_count as f64, &self.trades_min, &self.trades_max);

        // ── Technical ─────────────────────────────────────────────────────────
        opt_f64!(t.cu_limit.map(|v| v as f64), &self.cu_limit_min, &self.cu_limit_max);
        opt_f64!(t.cu_price.map(|v| v as f64), &self.cu_price_min, &self.cu_price_max);
        range_f64!(t.ix_labels_count as f64, &self.ix_count_min, &self.ix_count_max);

        if !self.ix_label.is_empty() {
            let needle = self.ix_label.to_lowercase();
            let matched = t.instruction_labels.as_array()
                .map(|arr| arr.iter().any(|v| {
                    v.as_str().map(|s| s.to_lowercase().contains(&needle)).unwrap_or(false)
                }))
                .unwrap_or(false);
            if !matched { return false; }
        }

        // ── Other ─────────────────────────────────────────────────────────────
        match self.migrated.as_str() {
            "yes" => if !t.is_migrated { return false; },
            "no"  => if  t.is_migrated { return false; },
            _     => {}
        }

        if !self.creator.is_empty() {
            let needle = self.creator.to_lowercase();
            if !t.creator_address.to_lowercase().contains(&needle) {
                return false;
            }
        }

        true
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
    let visible_cols   = use_state(load_visible_cols);
    let show_filters   = use_state(|| false);
    let filters        = use_state(Filters::default);
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

    // ── Open SSE for token deltas ─────────────────────────────────────────────
    {
        let token_state = token_state.clone();
        let live_val = *live;
        use_effect_with(live_val, move |live_val: &bool| {
            let es_opt = if *live_val {
                Some(connect_sse_tokens(token_state))
            } else {
                None
            };
            move || {
                if let Some(es) = es_opt {
                    es.close();
                }
            }
        });
    }

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
        use_effect_with((total, page_size, *page), move |(total, page_size, current_page)| {
            let total_pages = if *total == 0 { 1 } else { (*total + page_size - 1) / page_size };
            if *current_page > total_pages {
                page.set(total_pages);
            }
            || ()
        });
    }

    // ── Fetch ALL tokens on mount (client-side pagination/search/filters after) ──
    {
        let token_state = token_state.clone();
        use_effect_with((), move |_| {
            token_state.dispatch(TokenAction::SetLoading);
            spawn_local(async move {
                match fetch_tokens("", 5000, 0).await {
                    Ok(result) => {
                        token_state.dispatch(TokenAction::SetTokens { tokens: result.items, total: result.total });
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
                        token_state.dispatch(TokenAction::SetTokens { tokens: result.items, total: result.total });
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
        Callback::from(move |_: MouseEvent| {
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
                if new_cols.len() > 1 { new_cols.remove(&key); }
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

    // Each input immediately updates the active filter state (live filtering).
    let filters_for_input = filters.clone();
    let on_filter = move |field: &'static str| -> Callback<InputEvent> {
        let filters = filters_for_input.clone();
        Callback::from(move |e: InputEvent| {
            let el: web_sys::HtmlInputElement = e.target_unchecked_into();
            let val = el.value();
            let mut f = (*filters).clone();
            match field {
                "age_min"          => f.age_min          = val,
                "age_max"          => f.age_max          = val,
                "last_trade_min"   => f.last_trade_min   = val,
                "last_trade_max"   => f.last_trade_max   = val,
                "ath_age_min"      => f.ath_age_min      = val,
                "ath_age_max"      => f.ath_age_max      = val,
                "ath_fep_min"      => f.ath_fep_min      = val,
                "ath_fep_max"      => f.ath_fep_max      = val,
                "cur_fep_min"      => f.cur_fep_min      = val,
                "cur_fep_max"      => f.cur_fep_max      = val,
                "ath_price_min"    => f.ath_price_min    = val,
                "ath_price_max"    => f.ath_price_max    = val,
                "price_min"        => f.price_min        = val,
                "price_max"        => f.price_max        = val,
                "volume_min"       => f.volume_min       = val,
                "volume_max"       => f.volume_max       = val,
                "mcap_min"         => f.mcap_min         = val,
                "mcap_max"         => f.mcap_max         = val,
                "init_buy_min"     => f.init_buy_min     = val,
                "init_buy_max"     => f.init_buy_max     = val,
                "init_supply_min"  => f.init_supply_min  = val,
                "init_supply_max"  => f.init_supply_max  = val,
                "trades_min"       => f.trades_min       = val,
                "trades_max"       => f.trades_max       = val,
                "cu_limit_min"     => f.cu_limit_min     = val,
                "cu_limit_max"     => f.cu_limit_max     = val,
                "cu_price_min"     => f.cu_price_min     = val,
                "cu_price_max"     => f.cu_price_max     = val,
                "ix_count_min"     => f.ix_count_min     = val,
                "ix_count_max"     => f.ix_count_max     = val,
                "ix_label"         => f.ix_label         = val,
                "creator"          => f.creator          = val,
                _                  => {}
            }
            filters.set(f);
        })
    };

    let filters_for_select = filters.clone();
    let on_filter_select = move |field: &'static str| -> Callback<Event> {
        let filters = filters_for_select.clone();
        Callback::from(move |e: Event| {
            let el: web_sys::HtmlSelectElement = e.target_unchecked_into();
            let val = el.value();
            let mut f = (*filters).clone();
            match field {
                "migrated" => f.migrated = val,
                _          => {}
            }
            filters.set(f);
        })
    };

    let active_filter_count: usize = {
        let f = &*filters;
        [
            !f.age_min.is_empty()          || !f.age_max.is_empty(),
            !f.last_trade_min.is_empty()   || !f.last_trade_max.is_empty(),
            !f.ath_age_min.is_empty()      || !f.ath_age_max.is_empty(),
            !f.ath_fep_min.is_empty()      || !f.ath_fep_max.is_empty(),
            !f.cur_fep_min.is_empty()      || !f.cur_fep_max.is_empty(),
            !f.ath_price_min.is_empty()    || !f.ath_price_max.is_empty(),
            !f.price_min.is_empty()        || !f.price_max.is_empty(),
            !f.volume_min.is_empty()       || !f.volume_max.is_empty(),
            !f.mcap_min.is_empty()         || !f.mcap_max.is_empty(),
            !f.init_buy_min.is_empty()     || !f.init_buy_max.is_empty(),
            !f.init_supply_min.is_empty()  || !f.init_supply_max.is_empty(),
            !f.trades_min.is_empty()       || !f.trades_max.is_empty(),
            !f.cu_limit_min.is_empty()     || !f.cu_limit_max.is_empty(),
            !f.cu_price_min.is_empty()     || !f.cu_price_max.is_empty(),
            !f.ix_count_min.is_empty()     || !f.ix_count_max.is_empty(),
            !f.migrated.is_empty(),
            !f.ix_label.is_empty(),
            !f.creator.is_empty(),
        ].iter().filter(|&&b| b).count()
    };

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
    let total_pages = if effective_total == 0 { 1 } else { (effective_total + ps - 1) / ps };
    let cur_page = (*page).clamp(1, total_pages);
    let offset = cur_page.saturating_sub(1) * ps;

    // Slice to current page.
    displayed_tokens = displayed_tokens.into_iter().skip(offset).take(ps).collect();

    // Build visibility mask (one bool per COLUMNS entry, in order)
    let vis: Vec<bool> = COLUMNS.iter()
        .map(|(key, _, _, _)| (*visible_cols).contains(*key))
        .collect();
    // total rendered columns: # + visible cols + action
    let num_cols = 2 + vis.iter().filter(|&&b| b).count();

    // Build headers — skip hidden columns
    let mut headers_html = vec![html! { <th class="th-row-num">{ "#" }</th> }];
    for (i, &(field, label, _, th_cls)) in COLUMNS.iter().enumerate() {
        if !vis[i] { continue; }
        let is_sorted = token_state.sort.field == field;
        let sort_icon = if is_sorted {
            match token_state.sort.order {
                SortOrder::Asc  => "↑",
                SortOrder::Desc => "↓",
            }
        } else { "" };
        let on_click = {
            let on_toggle_sort = on_toggle_sort.clone();
            let field = field.to_string();
            Callback::from(move |_: MouseEvent| on_toggle_sort.emit(field.clone()))
        };
        headers_html.push(html! {
            <th class={classes!(th_cls)}>
                <button
                    class={classes!("sort-header-btn", is_sorted.then_some("sort-active"))}
                    onclick={on_click}
                    title={format!("Sort by {}", label)}
                >
                    { label }
                    { if is_sorted { html! { <span class="sort-icon">{ sort_icon }</span> } } else { html! {} } }
                </button>
            </th>
        });
    }
    headers_html.push(html! { <th class="th-action"></th> });

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
                        <button
                            class={classes!("live-toggle-btn", (*live).then_some("live-toggle-active"))}
                            onclick={on_toggle_live}
                            title={if *live { "Pause live updates" } else { "Resume live updates" }}
                        >
                            <span class={if *live { "live-dot" } else { "live-dot-off" }}></span>
                            { if *live { "LIVE" } else { "PAUSED" } }
                        </button>
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
                    <div class="col-options-panel">
                        <div class="col-options-header">{ "COLUMN OPTIONS" }</div>
                        <div class="col-options-grid">
                            { for COLUMNS.iter().map(|&(key, label, _, _)| {
                                let checked = (*visible_cols).contains(key);
                                let key_str = key.to_string();
                                let on_change = {
                                    let on_toggle_col = on_toggle_col.clone();
                                    Callback::from(move |_: Event| on_toggle_col.emit(key_str.clone()))
                                };
                                html! {
                                    <label class="col-opt-item">
                                        <input type="checkbox" checked={checked} onchange={on_change} />
                                        <span>{ label }</span>
                                    </label>
                                }
                            }) }
                        </div>
                    </div>
                }

                if *show_filters {
                    <div class="filter-panel">
                        <div class="filter-panel-header">
                            <span class="filter-panel-title">{ "FILTERS" }</span>
                            if active_filter_count > 0 {
                                <button class="filter-clear-btn" onclick={on_clear_filters.clone()}>
                                    { format!("Clear all ({})", active_filter_count) }
                                </button>
                            }
                        </div>

                        // ── Time ──────────────────────────────────────────────
                        <div class="filter-group">
                            <div class="filter-group-label">{ "Time" }</div>
                            <div class="filter-group-body">
                                <div class="filter-item">
                                    <span class="filter-label">{ "Age (h)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                            value={filters.age_min.clone()} oninput={on_filter("age_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                            value={filters.age_max.clone()} oninput={on_filter("age_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Last Trade (h)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                            value={filters.last_trade_min.clone()} oninput={on_filter("last_trade_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                            value={filters.last_trade_max.clone()} oninput={on_filter("last_trade_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "ATH Age (h)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                            value={filters.ath_age_min.clone()} oninput={on_filter("ath_age_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                            value={filters.ath_age_max.clone()} oninput={on_filter("ath_age_max")} />
                                    </div>
                                </div>
                            </div>
                        </div>

                        // ── Performance ───────────────────────────────────────
                        <div class="filter-group">
                            <div class="filter-group-label">{ "Performance" }</div>
                            <div class="filter-group-body">
                                <div class="filter-item">
                                    <span class="filter-label">{ "ATH/FEP (×)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                            value={filters.ath_fep_min.clone()} oninput={on_filter("ath_fep_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                            value={filters.ath_fep_max.clone()} oninput={on_filter("ath_fep_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Cur/FEP (×)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                            value={filters.cur_fep_min.clone()} oninput={on_filter("cur_fep_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                            value={filters.cur_fep_max.clone()} oninput={on_filter("cur_fep_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "ATH Price" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="any" placeholder="min"
                                            value={filters.ath_price_min.clone()} oninput={on_filter("ath_price_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="any" placeholder="max"
                                            value={filters.ath_price_max.clone()} oninput={on_filter("ath_price_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Price" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="any" placeholder="min"
                                            value={filters.price_min.clone()} oninput={on_filter("price_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="any" placeholder="max"
                                            value={filters.price_max.clone()} oninput={on_filter("price_max")} />
                                    </div>
                                </div>
                            </div>
                        </div>

                        // ── Market ────────────────────────────────────────────
                        <div class="filter-group">
                            <div class="filter-group-label">{ "Market" }</div>
                            <div class="filter-group-body">
                                <div class="filter-item">
                                    <span class="filter-label">{ "Volume (SOL)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.01" placeholder="min"
                                            value={filters.volume_min.clone()} oninput={on_filter("volume_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.01" placeholder="max"
                                            value={filters.volume_max.clone()} oninput={on_filter("volume_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "MCap (SOL)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.01" placeholder="min"
                                            value={filters.mcap_min.clone()} oninput={on_filter("mcap_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.01" placeholder="max"
                                            value={filters.mcap_max.clone()} oninput={on_filter("mcap_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Init Buy (SOL)" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="0.001" placeholder="min"
                                            value={filters.init_buy_min.clone()} oninput={on_filter("init_buy_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="0.001" placeholder="max"
                                            value={filters.init_buy_max.clone()} oninput={on_filter("init_buy_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Init Supply" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                            value={filters.init_supply_min.clone()} oninput={on_filter("init_supply_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                            value={filters.init_supply_max.clone()} oninput={on_filter("init_supply_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Trades" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                            value={filters.trades_min.clone()} oninput={on_filter("trades_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                            value={filters.trades_max.clone()} oninput={on_filter("trades_max")} />
                                    </div>
                                </div>
                            </div>
                        </div>

                        // ── Technical ─────────────────────────────────────────
                        <div class="filter-group">
                            <div class="filter-group-label">{ "Technical" }</div>
                            <div class="filter-group-body">
                                <div class="filter-item">
                                    <span class="filter-label">{ "CU Limit" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                            value={filters.cu_limit_min.clone()} oninput={on_filter("cu_limit_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                            value={filters.cu_limit_max.clone()} oninput={on_filter("cu_limit_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "CU Price" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                            value={filters.cu_price_min.clone()} oninput={on_filter("cu_price_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                            value={filters.cu_price_max.clone()} oninput={on_filter("cu_price_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "IX Count" }</span>
                                    <div class="filter-range">
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                            value={filters.ix_count_min.clone()} oninput={on_filter("ix_count_min")} />
                                        <span class="filter-sep">{ "–" }</span>
                                        <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                            value={filters.ix_count_max.clone()} oninput={on_filter("ix_count_max")} />
                                    </div>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "IX Label" }</span>
                                    <input class="filter-input filter-input-wide" type="text"
                                        placeholder="Jito, BuyExact…"
                                        value={filters.ix_label.clone()} oninput={on_filter("ix_label")} />
                                </div>
                            </div>
                        </div>

                        // ── Other ─────────────────────────────────────────────
                        <div class="filter-group">
                            <div class="filter-group-label">{ "Other" }</div>
                            <div class="filter-group-body">
                                <div class="filter-item">
                                    <span class="filter-label">{ "Migrated" }</span>
                                    <select class="filter-select" onchange={on_filter_select("migrated")}>
                                        <option value="" selected={filters.migrated.is_empty()}>{ "All" }</option>
                                        <option value="yes" selected={filters.migrated == "yes"}>{ "Yes" }</option>
                                        <option value="no"  selected={filters.migrated == "no"}>{ "No" }</option>
                                    </select>
                                </div>
                                <div class="filter-item">
                                    <span class="filter-label">{ "Creator" }</span>
                                    <input class="filter-input filter-input-wide" type="text"
                                        placeholder="address substring…"
                                        value={filters.creator.clone()} oninput={on_filter("creator")} />
                                </div>
                            </div>
                        </div>
                    </div>
                }

                if token_state.loading {
                    <p class="loading">{ "Loading tokens…" }</p>
                } else if let Some(err) = &token_state.error {
                    <p class="error">{ err }</p>
                } else {
                    <div class="table-wrapper">
                        <table class="trade-table">
                            <colgroup>
                                <col style="width: 40px;" />
                                { for COLUMNS.iter().enumerate().filter_map(|(i, &(_, _, w, _))| {
                                    if vis[i] { Some(html! { <col style={format!("width: {}px;", w)} /> }) }
                                    else { None }
                                }) }
                                <col style="width: 48px;" />
                            </colgroup>
                            <thead>
                                <tr>
                                    { for headers_html }
                                </tr>
                            </thead>
                            <tbody>
                                { if displayed_tokens.is_empty() {
                                        html! {
                                        <tr>
                                            <td class="no-data" colspan={num_cols.to_string()}>{ "No tokens found." }</td>
                                        </tr>
                                    }
                                } else {
                                    html! {
                                        { for displayed_tokens.iter().enumerate().map(|(idx, token)| {
                                            let row_num = offset + idx + 1;
                                            let selected = token_state.selected_mint.as_deref() == Some(&token.mint_address);
                                            let detail = if selected { (*selected_detail).clone() } else { None };
                                            html! {
                                                <TokenRow
                                                    key={token.mint_address.clone()}
                                                    token={token.clone()}
                                                    selected={selected}
                                                    detail={detail}
                                                    detail_loading={*detail_loading && selected}
                                                    detail_error={(*detail_error).clone()}
                                                    on_select={on_select_token.clone()}
                                                    row_num={Some(row_num)}
                                                    visible_cols={vis.clone()}
                                                />
                                            }
                                        }) }
                                    }
                                } }
                            </tbody>
                        </table>
                    </div>
                }
            </main>
        </div>
    }
}
