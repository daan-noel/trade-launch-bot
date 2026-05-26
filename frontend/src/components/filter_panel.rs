use js_sys;
use web_sys;
use yew::prelude::*;

use crate::services::api::TokenRecord;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse an ISO-8601 datetime string and return how many hours ago it was.
pub fn iso_hours_ago(s: &str) -> Option<f64> {
    let ts_ms = js_sys::Date::parse(s);
    if ts_ms.is_nan() {
        return None;
    }
    let now_ms = js_sys::Date::now();
    Some((now_ms - ts_ms).max(0.0) / 3_600_000.0)
}

// ── Filters ───────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
pub struct Filters {
    // ── Time ──────────────────────────────────────────────────────────────────
    pub age_min:          String,
    pub age_max:          String,
    pub last_trade_min:   String,
    pub last_trade_max:   String,
    pub ath_age_min:      String,
    pub ath_age_max:      String,
    // ── Performance ───────────────────────────────────────────────────────────
    pub ath_fep_min:      String,
    pub ath_fep_max:      String,
    pub cur_fep_min:      String,
    pub cur_fep_max:      String,
    pub ath_price_min:    String,
    pub ath_price_max:    String,
    pub price_min:        String,
    pub price_max:        String,
    // ── Market ────────────────────────────────────────────────────────────────
    pub volume_min:       String,
    pub volume_max:       String,
    pub mcap_min:         String,
    pub mcap_max:         String,
    pub init_buy_min:     String,
    pub init_buy_max:     String,
    pub init_supply_min:  String,
    pub init_supply_max:  String,
    pub token_amount_min: String,
    pub token_amount_max: String,
    pub max_sol_cost_min: String,
    pub max_sol_cost_max: String,
    pub spendable_sol_in_min: String,
    pub spendable_sol_in_max: String,
    pub min_tokens_out_min: String,
    pub min_tokens_out_max: String,
    pub trades_min:       String,
    pub trades_max:       String,
    // ── Technical ─────────────────────────────────────────────────────────────
    pub cu_limit_min:     String,
    pub cu_limit_max:     String,
    pub cu_price_min:     String,
    pub cu_price_max:     String,
    pub ix_count_min:     String,
    pub ix_count_max:     String,
    pub ix_label:         String,
    // ── Other ─────────────────────────────────────────────────────────────────
    pub migrated:         String,
    pub creator:          String,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        self.age_min.is_empty()             && self.age_max.is_empty()
        && self.last_trade_min.is_empty()   && self.last_trade_max.is_empty()
        && self.ath_age_min.is_empty()      && self.ath_age_max.is_empty()
        && self.ath_fep_min.is_empty()      && self.ath_fep_max.is_empty()
        && self.cur_fep_min.is_empty()      && self.cur_fep_max.is_empty()
        && self.ath_price_min.is_empty()    && self.ath_price_max.is_empty()
        && self.price_min.is_empty()        && self.price_max.is_empty()
        && self.volume_min.is_empty()       && self.volume_max.is_empty()
        && self.mcap_min.is_empty()         && self.mcap_max.is_empty()
        && self.init_buy_min.is_empty()     && self.init_buy_max.is_empty()
        && self.init_supply_min.is_empty()  && self.init_supply_max.is_empty()
        && self.token_amount_min.is_empty() && self.token_amount_max.is_empty()
        && self.max_sol_cost_min.is_empty() && self.max_sol_cost_max.is_empty()
        && self.spendable_sol_in_min.is_empty() && self.spendable_sol_in_max.is_empty()
        && self.min_tokens_out_min.is_empty() && self.min_tokens_out_max.is_empty()
        && self.trades_min.is_empty()       && self.trades_max.is_empty()
        && self.cu_limit_min.is_empty()     && self.cu_limit_max.is_empty()
        && self.cu_price_min.is_empty()     && self.cu_price_max.is_empty()
        && self.ix_count_min.is_empty()     && self.ix_count_max.is_empty()
        && self.migrated.is_empty()
        && self.ix_label.is_empty()
        && self.creator.is_empty()
    }

    pub fn active_count(&self) -> usize {
        [
            !self.age_min.is_empty()              || !self.age_max.is_empty(),
            !self.last_trade_min.is_empty()        || !self.last_trade_max.is_empty(),
            !self.ath_age_min.is_empty()           || !self.ath_age_max.is_empty(),
            !self.ath_fep_min.is_empty()           || !self.ath_fep_max.is_empty(),
            !self.cur_fep_min.is_empty()           || !self.cur_fep_max.is_empty(),
            !self.ath_price_min.is_empty()         || !self.ath_price_max.is_empty(),
            !self.price_min.is_empty()             || !self.price_max.is_empty(),
            !self.volume_min.is_empty()            || !self.volume_max.is_empty(),
            !self.mcap_min.is_empty()              || !self.mcap_max.is_empty(),
            !self.init_buy_min.is_empty()          || !self.init_buy_max.is_empty(),
            !self.init_supply_min.is_empty()       || !self.init_supply_max.is_empty(),
            !self.token_amount_min.is_empty()      || !self.token_amount_max.is_empty(),
            !self.max_sol_cost_min.is_empty()      || !self.max_sol_cost_max.is_empty(),
            !self.spendable_sol_in_min.is_empty()  || !self.spendable_sol_in_max.is_empty(),
            !self.min_tokens_out_min.is_empty()    || !self.min_tokens_out_max.is_empty(),
            !self.trades_min.is_empty()            || !self.trades_max.is_empty(),
            !self.cu_limit_min.is_empty()          || !self.cu_limit_max.is_empty(),
            !self.cu_price_min.is_empty()          || !self.cu_price_max.is_empty(),
            !self.ix_count_min.is_empty()          || !self.ix_count_max.is_empty(),
            !self.migrated.is_empty(),
            !self.ix_label.is_empty(),
            !self.creator.is_empty(),
        ]
        .iter()
        .filter(|&&b| b)
        .count()
    }

    pub fn passes(&self, t: &TokenRecord) -> bool {
        macro_rules! range_f64 {
            ($val:expr, $min:expr, $max:expr) => {{
                let val: f64 = $val;
                if let Ok(v) = $min.parse::<f64>() { if val < v { return false; } }
                if let Ok(v) = $max.parse::<f64>() { if val > v { return false; } }
            }};
        }
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

        opt_f64!(ath_fep,         &self.ath_fep_min,   &self.ath_fep_max);
        opt_f64!(cur_fep,         &self.cur_fep_min,   &self.cur_fep_max);
        opt_f64!(t.ath_price,     &self.ath_price_min, &self.ath_price_max);
        opt_f64!(t.current_price, &self.price_min,     &self.price_max);

        range_f64!(t.volume_sol_total, &self.volume_min, &self.volume_max);
        opt_f64!(t.market_cap,                          &self.mcap_min,           &self.mcap_max);
        opt_f64!(t.initial_buy_sol,                     &self.init_buy_min,       &self.init_buy_max);
        opt_f64!(t.initial_supply_token.map(|v| v as f64), &self.init_supply_min, &self.init_supply_max);
        opt_f64!(t.token_amount.map(|v| v as f64),      &self.token_amount_min,   &self.token_amount_max);
        opt_f64!(t.max_sol_cost.map(|v| v as f64),      &self.max_sol_cost_min,   &self.max_sol_cost_max);
        opt_f64!(t.spendable_sol_in.map(|v| v as f64),  &self.spendable_sol_in_min, &self.spendable_sol_in_max);
        opt_f64!(t.min_tokens_out.map(|v| v as f64),    &self.min_tokens_out_min, &self.min_tokens_out_max);
        range_f64!(t.trade_count as f64, &self.trades_min, &self.trades_max);

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

// ── Component ─────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct FilterPanelProps {
    pub filters: Filters,
    pub active_filter_count: usize,
    pub on_clear: Callback<MouseEvent>,
    /// Emits (field_name, new_value) for text/number inputs.
    pub on_change: Callback<(String, String)>,
    /// Emits (field_name, new_value) for select inputs.
    pub on_select_change: Callback<(String, String)>,
}

#[function_component(FilterPanel)]
pub fn filter_panel(props: &FilterPanelProps) -> Html {
    let on_change = props.on_change.clone();
    let on_select_change = props.on_select_change.clone();

    let make_input_cb = move |field: &'static str| -> Callback<InputEvent> {
        let cb = on_change.clone();
        Callback::from(move |e: InputEvent| {
            let el: web_sys::HtmlInputElement = e.target_unchecked_into();
            cb.emit((field.to_string(), el.value()));
        })
    };
    let make_select_cb = move |field: &'static str| -> Callback<Event> {
        let cb = on_select_change.clone();
        Callback::from(move |e: Event| {
            let el: web_sys::HtmlSelectElement = e.target_unchecked_into();
            cb.emit((field.to_string(), el.value()));
        })
    };

    let f = &props.filters;
    let active = props.active_filter_count;

    html! {
        <div class="filter-panel">
            <div class="filter-panel-header">
                <span class="filter-panel-title">{ "FILTERS" }</span>
                if active > 0 {
                    <button class="filter-clear-btn" onclick={props.on_clear.clone()}>
                        { format!("Clear all ({})", active) }
                    </button>
                }
            </div>

            <div class="filter-group">
                <div class="filter-group-label">{ "Time" }</div>
                <div class="filter-group-body">
                    <div class="filter-item">
                        <span class="filter-label">{ "Age (h)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                value={f.age_min.clone()} oninput={make_input_cb("age_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                value={f.age_max.clone()} oninput={make_input_cb("age_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Last Trade (h)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                value={f.last_trade_min.clone()} oninput={make_input_cb("last_trade_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                value={f.last_trade_max.clone()} oninput={make_input_cb("last_trade_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "ATH Age (h)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                value={f.ath_age_min.clone()} oninput={make_input_cb("ath_age_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                value={f.ath_age_max.clone()} oninput={make_input_cb("ath_age_max")} />
                        </div>
                    </div>
                </div>
            </div>

            <div class="filter-group">
                <div class="filter-group-label">{ "Performance" }</div>
                <div class="filter-group-body">
                    <div class="filter-item">
                        <span class="filter-label">{ "ATH/FEP (×)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                value={f.ath_fep_min.clone()} oninput={make_input_cb("ath_fep_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                value={f.ath_fep_max.clone()} oninput={make_input_cb("ath_fep_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Cur/FEP (×)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="min"
                                value={f.cur_fep_min.clone()} oninput={make_input_cb("cur_fep_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.1" placeholder="max"
                                value={f.cur_fep_max.clone()} oninput={make_input_cb("cur_fep_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "ATH Price" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="any" placeholder="min"
                                value={f.ath_price_min.clone()} oninput={make_input_cb("ath_price_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="any" placeholder="max"
                                value={f.ath_price_max.clone()} oninput={make_input_cb("ath_price_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Price" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="any" placeholder="min"
                                value={f.price_min.clone()} oninput={make_input_cb("price_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="any" placeholder="max"
                                value={f.price_max.clone()} oninput={make_input_cb("price_max")} />
                        </div>
                    </div>
                </div>
            </div>

            <div class="filter-group">
                <div class="filter-group-label">{ "Market" }</div>
                <div class="filter-group-body">
                    <div class="filter-item">
                        <span class="filter-label">{ "Volume (SOL)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.01" placeholder="min"
                                value={f.volume_min.clone()} oninput={make_input_cb("volume_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.01" placeholder="max"
                                value={f.volume_max.clone()} oninput={make_input_cb("volume_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "MCap (SOL)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.01" placeholder="min"
                                value={f.mcap_min.clone()} oninput={make_input_cb("mcap_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.01" placeholder="max"
                                value={f.mcap_max.clone()} oninput={make_input_cb("mcap_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Init Buy (SOL)" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="0.001" placeholder="min"
                                value={f.init_buy_min.clone()} oninput={make_input_cb("init_buy_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="0.001" placeholder="max"
                                value={f.init_buy_max.clone()} oninput={make_input_cb("init_buy_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Init Supply" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.init_supply_min.clone()} oninput={make_input_cb("init_supply_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.init_supply_max.clone()} oninput={make_input_cb("init_supply_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Token Amount" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.token_amount_min.clone()} oninput={make_input_cb("token_amount_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.token_amount_max.clone()} oninput={make_input_cb("token_amount_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Max SOL Cost" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.max_sol_cost_min.clone()} oninput={make_input_cb("max_sol_cost_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.max_sol_cost_max.clone()} oninput={make_input_cb("max_sol_cost_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Spendable SOL In" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.spendable_sol_in_min.clone()} oninput={make_input_cb("spendable_sol_in_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.spendable_sol_in_max.clone()} oninput={make_input_cb("spendable_sol_in_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Min Tokens Out" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.min_tokens_out_min.clone()} oninput={make_input_cb("min_tokens_out_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.min_tokens_out_max.clone()} oninput={make_input_cb("min_tokens_out_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Trades" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.trades_min.clone()} oninput={make_input_cb("trades_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.trades_max.clone()} oninput={make_input_cb("trades_max")} />
                        </div>
                    </div>
                </div>
            </div>

            <div class="filter-group">
                <div class="filter-group-label">{ "Technical" }</div>
                <div class="filter-group-body">
                    <div class="filter-item">
                        <span class="filter-label">{ "CU Limit" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.cu_limit_min.clone()} oninput={make_input_cb("cu_limit_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.cu_limit_max.clone()} oninput={make_input_cb("cu_limit_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "CU Price" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.cu_price_min.clone()} oninput={make_input_cb("cu_price_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.cu_price_max.clone()} oninput={make_input_cb("cu_price_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "IX Count" }</span>
                        <div class="filter-range">
                            <input class="filter-input" type="number" min="0" step="1" placeholder="min"
                                value={f.ix_count_min.clone()} oninput={make_input_cb("ix_count_min")} />
                            <span class="filter-sep">{ "–" }</span>
                            <input class="filter-input" type="number" min="0" step="1" placeholder="max"
                                value={f.ix_count_max.clone()} oninput={make_input_cb("ix_count_max")} />
                        </div>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "IX Label" }</span>
                        <input class="filter-input filter-input-wide" type="text"
                            placeholder="Jito, BuyExact…"
                            value={f.ix_label.clone()} oninput={make_input_cb("ix_label")} />
                    </div>
                </div>
            </div>

            <div class="filter-group">
                <div class="filter-group-label">{ "Other" }</div>
                <div class="filter-group-body">
                    <div class="filter-item">
                        <span class="filter-label">{ "Migrated" }</span>
                        <select class="filter-select" onchange={make_select_cb("migrated")}>
                            <option value="" selected={f.migrated.is_empty()}>{ "All" }</option>
                            <option value="yes" selected={f.migrated == "yes"}>{ "Yes" }</option>
                            <option value="no"  selected={f.migrated == "no"}>{ "No" }</option>
                        </select>
                    </div>
                    <div class="filter-item">
                        <span class="filter-label">{ "Creator" }</span>
                        <input class="filter-input filter-input-wide" type="text"
                            placeholder="address substring…"
                            value={f.creator.clone()} oninput={make_input_cb("creator")} />
                    </div>
                </div>
            </div>
        </div>
    }
}
