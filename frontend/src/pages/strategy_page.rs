use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use serde_json::Value;

use crate::components::Header;
use crate::components::modal::Modal;
use crate::services::api::{
    create_tpsl_rule, delete_tpsl_rule, fetch_tpsl_rules, simulate_tpsl_rule,
    update_tpsl_rule, CreateRuleRequest, RuleRecord, SimulationResultRecord,
    UpdateRuleRequest,
};
use crate::utils::format::{format_age, format_compact, format_price};

// ── Modal mode ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum ModalMode {
    None,
    Add,
    Edit(RuleRecord),
}

// ── Page ──────────────────────────────────────────────────────────────────────

#[function_component(StrategyPage)]
pub fn strategy_page() -> Html {
    // ── Data ──────────────────────────────────────────────────────────────────
    let rules = use_state(Vec::<RuleRecord>::new);
    let loading = use_state(|| false);
    let load_error = use_state(|| Option::<String>::None);
    let search = use_state(String::new);

    // ── Modal / form ──────────────────────────────────────────────────────────
    let modal_mode = use_state(|| ModalMode::None);
    let f_name = use_state(String::new);
    let f_initial_buy = use_state(String::new);
    let f_cu_limit = use_state(String::new);
    let f_cu_price = use_state(String::new);
    let f_ix_labels = use_state(String::new);
    let f_buy_amount = use_state(String::new);
    let f_take_profit = use_state(String::new);
    let f_stop_loss = use_state(String::new);
    let form_error = use_state(|| Option::<String>::None);
    let form_loading = use_state(|| false);

    // ── Delete confirm ────────────────────────────────────────────────────────
    let confirm_delete_id = use_state(|| Option::<String>::None);
    let delete_loading = use_state(|| false);

    // ── Simulation ────────────────────────────────────────────────────────────
    let simulate_result = use_state(|| Option::<SimulationResultRecord>::None);
    let simulate_error = use_state(|| Option::<String>::None);
    let simulate_loading = use_state(|| false);

    // ── Load rules on mount ───────────────────────────────────────────────────
    {
        let rules = rules.clone();
        let load_error = load_error.clone();
        let loading = loading.clone();
        use_effect_with((), move |_| {
            loading.set(true);
            spawn_local(async move {
                match fetch_tpsl_rules().await {
                    Ok(fetched) => rules.set(fetched),
                    Err(err) => load_error.set(Some(err)),
                }
                loading.set(false);
            });
            || ()
        });
    }

    // ── Helpers: open modals ──────────────────────────────────────────────────
    let open_add = {
        let (modal_mode, f_name, f_initial_buy, f_cu_limit, f_cu_price) = (
            modal_mode.clone(), f_name.clone(), f_initial_buy.clone(),
            f_cu_limit.clone(), f_cu_price.clone(),
        );
        let (f_ix_labels, f_buy_amount, f_take_profit, f_stop_loss, form_error) = (
            f_ix_labels.clone(), f_buy_amount.clone(), f_take_profit.clone(),
            f_stop_loss.clone(), form_error.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            f_name.set(String::new());
            f_initial_buy.set(String::new());
            f_cu_limit.set(String::new());
            f_cu_price.set(String::new());
            f_ix_labels.set(String::new());
            f_buy_amount.set(String::new());
            f_take_profit.set(String::new());
            f_stop_loss.set(String::new());
            form_error.set(None);
            modal_mode.set(ModalMode::Add);
        })
    };

    let open_edit = {
        let (modal_mode, f_name, f_initial_buy, f_cu_limit, f_cu_price) = (
            modal_mode.clone(), f_name.clone(), f_initial_buy.clone(),
            f_cu_limit.clone(), f_cu_price.clone(),
        );
        let (f_ix_labels, f_buy_amount, f_take_profit, f_stop_loss, form_error) = (
            f_ix_labels.clone(), f_buy_amount.clone(), f_take_profit.clone(),
            f_stop_loss.clone(), form_error.clone(),
        );
        Callback::from(move |rule: RuleRecord| {
            f_name.set(rule.rule_name.clone());
            f_initial_buy.set(rule.p_initial_buy_sol.to_string());
            f_cu_limit.set(rule.p_cu_limit.map(|v| v.to_string()).unwrap_or_default());
            f_cu_price.set(rule.p_cu_price.map(|v| v.to_string()).unwrap_or_default());
            let labels = rule.p_ix_labels
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            f_ix_labels.set(labels);
            f_buy_amount.set(rule.buy_amount.to_string());
            f_take_profit.set(rule.take_profit.to_string());
            f_stop_loss.set(rule.stop_loss.to_string());
            form_error.set(None);
            modal_mode.set(ModalMode::Edit(rule));
        })
    };

    let close_modal: Callback<()> = {
        let modal_mode = modal_mode.clone();
        Callback::from(move |_: ()| modal_mode.set(ModalMode::None))
    };
    let cancel_modal: Callback<MouseEvent> = {
        let modal_mode = modal_mode.clone();
        Callback::from(move |_: MouseEvent| modal_mode.set(ModalMode::None))
    };

    // ── Save (create or update) ───────────────────────────────────────────────
    let on_save = {
        let (modal_mode, rules, form_error, form_loading) = (
            modal_mode.clone(), rules.clone(), form_error.clone(), form_loading.clone(),
        );
        let (f_name, f_initial_buy, f_cu_limit, f_cu_price, f_ix_labels) = (
            f_name.clone(), f_initial_buy.clone(), f_cu_limit.clone(),
            f_cu_price.clone(), f_ix_labels.clone(),
        );
        let (f_buy_amount, f_take_profit, f_stop_loss) = (
            f_buy_amount.clone(), f_take_profit.clone(), f_stop_loss.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let mode = (*modal_mode).clone();
            let (rules, form_error, form_loading, modal_mode) = (
                rules.clone(), form_error.clone(), form_loading.clone(), modal_mode.clone(),
            );
            let (name, initial_buy_s, cu_limit_s, cu_price_s, ix_labels_s) = (
                (*f_name).clone(), (*f_initial_buy).clone(), (*f_cu_limit).clone(),
                (*f_cu_price).clone(), (*f_ix_labels).clone(),
            );
            let (buy_amount_s, take_profit_s, stop_loss_s) = (
                (*f_buy_amount).clone(), (*f_take_profit).clone(), (*f_stop_loss).clone(),
            );
            form_error.set(None);
            form_loading.set(true);

            spawn_local(async move {
                let buy_amount = match buy_amount_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => { form_error.set(Some("Invalid buy amount".into())); form_loading.set(false); return; }
                };
                let take_profit = match take_profit_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => { form_error.set(Some("Invalid take profit %".into())); form_loading.set(false); return; }
                };
                let stop_loss = match stop_loss_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => { form_error.set(Some("Invalid stop loss %".into())); form_loading.set(false); return; }
                };

                match mode {
                    ModalMode::Add => {
                        let p_initial_buy_sol = match initial_buy_s.trim().parse::<f64>() {
                            Ok(v) => v,
                            Err(_) => { form_error.set(Some("Invalid initial buy SOL".into())); form_loading.set(false); return; }
                        };
                        let p_cu_limit = if cu_limit_s.trim().is_empty() { None } else { cu_limit_s.trim().parse::<u64>().ok() };
                        let p_cu_price = if cu_price_s.trim().is_empty() { None } else { cu_price_s.trim().parse::<u64>().ok() };
                        let labels: Vec<Value> = ix_labels_s
                            .split(',').map(|s| s.trim()).filter(|s| !s.is_empty())
                            .map(|s| Value::String(s.to_string())).collect();
                        let req = CreateRuleRequest {
                            rule_name: name, p_initial_buy_sol, p_cu_limit, p_cu_price,
                            p_ix_labels: Value::Array(labels), buy_amount, take_profit, stop_loss,
                        };
                        match create_tpsl_rule(&req).await {
                            Ok(new_rule) => {
                                let mut items = (*rules).clone();
                                items.insert(0, new_rule);
                                rules.set(items);
                                modal_mode.set(ModalMode::None);
                            }
                            Err(err) => form_error.set(Some(err)),
                        }
                    }
                    ModalMode::Edit(rule) => {
                        let req = UpdateRuleRequest {
                            rule_name: Some(name), buy_amount: Some(buy_amount),
                            take_profit: Some(take_profit), stop_loss: Some(stop_loss),
                            is_active: None,
                        };
                        match update_tpsl_rule(&rule.id, &req).await {
                            Ok(updated) => {
                                let items = (*rules).iter()
                                    .map(|r| if r.id == updated.id { updated.clone() } else { r.clone() })
                                    .collect();
                                rules.set(items);
                                modal_mode.set(ModalMode::None);
                            }
                            Err(err) => form_error.set(Some(err)),
                        }
                    }
                    ModalMode::None => {}
                }
                form_loading.set(false);
            });
        })
    };

    // ── Toggle active ─────────────────────────────────────────────────────────
    let on_toggle_active = {
        let rules = rules.clone();
        Callback::from(move |rule: RuleRecord| {
            let rules = rules.clone();
            spawn_local(async move {
                let req = UpdateRuleRequest {
                    rule_name: None, buy_amount: None, take_profit: None, stop_loss: None,
                    is_active: Some(!rule.is_active),
                };
                if let Ok(updated) = update_tpsl_rule(&rule.id, &req).await {
                    let items = (*rules).iter()
                        .map(|r| if r.id == updated.id { updated.clone() } else { r.clone() })
                        .collect();
                    rules.set(items);
                }
            });
        })
    };

    // ── Delete flow ───────────────────────────────────────────────────────────
    let on_request_delete = {
        let confirm_delete_id = confirm_delete_id.clone();
        Callback::from(move |rule_id: String| confirm_delete_id.set(Some(rule_id)))
    };
    let on_cancel_delete = {
        let confirm_delete_id = confirm_delete_id.clone();
        Callback::from(move |_: MouseEvent| confirm_delete_id.set(None))
    };
    let on_confirm_delete = {
        let (confirm_delete_id, rules, delete_loading) = (
            confirm_delete_id.clone(), rules.clone(), delete_loading.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let rule_id = match (*confirm_delete_id).clone() { Some(id) => id, None => return };
            let (confirm_delete_id, rules, delete_loading) = (
                confirm_delete_id.clone(), rules.clone(), delete_loading.clone(),
            );
            delete_loading.set(true);
            spawn_local(async move {
                if delete_tpsl_rule(&rule_id).await.is_ok() {
                    let items = (*rules).iter().filter(|r| r.id != rule_id).cloned().collect();
                    rules.set(items);
                }
                confirm_delete_id.set(None);
                delete_loading.set(false);
            });
        })
    };

    // ── Simulate ──────────────────────────────────────────────────────────────
    let on_simulate = {
        let (simulate_result, simulate_error, simulate_loading) = (
            simulate_result.clone(), simulate_error.clone(), simulate_loading.clone(),
        );
        Callback::from(move |rule_id: String| {
            let (simulate_result, simulate_error, simulate_loading) = (
                simulate_result.clone(), simulate_error.clone(), simulate_loading.clone(),
            );
            simulate_result.set(None);
            simulate_error.set(None);
            simulate_loading.set(true);
            spawn_local(async move {
                match simulate_tpsl_rule(&rule_id).await {
                    Ok(result) => simulate_result.set(Some(result)),
                    Err(err) => simulate_error.set(Some(err)),
                }
                simulate_loading.set(false);
            });
        })
    };

    let search_val = (*search).to_lowercase();
    let filtered: Vec<&RuleRecord> = (*rules).iter()
        .filter(|r| search_val.is_empty() || r.rule_name.to_lowercase().contains(&search_val))
        .collect();

    // ── Build table rows ──────────────────────────────────────────────────────
    let rule_rows = filtered.iter().map(|rule| {
        let rule = (*rule).clone();

        let on_edit_cb = {
            let open_edit = open_edit.clone();
            let rule = rule.clone();
            Callback::from(move |_: MouseEvent| open_edit.emit(rule.clone()))
        };
        let on_delete_cb = {
            let on_request_delete = on_request_delete.clone();
            let rule_id = rule.id.clone();
            Callback::from(move |_: MouseEvent| on_request_delete.emit(rule_id.clone()))
        };
        let on_sim_cb = {
            let on_simulate = on_simulate.clone();
            let rule_id = rule.id.clone();
            Callback::from(move |_: MouseEvent| on_simulate.emit(rule_id.clone()))
        };
        let on_toggle_cb = {
            let on_toggle_active = on_toggle_active.clone();
            let rule = rule.clone();
            Callback::from(move |_: MouseEvent| on_toggle_active.emit(rule.clone()))
        };

        let is_confirming = (*confirm_delete_id).as_ref().map(|id| id == &rule.id).unwrap_or(false);

        let labels_display = rule.p_ix_labels.as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".to_string());

        html! {
            <tr key={rule.id.clone()}>
                <td>
                    <span class="rule-name-cell">{ &rule.rule_name }</span>
                </td>
                <td class="num-col">{ format!("{:.3}", rule.p_initial_buy_sol) }</td>
                <td class="dim-col">{ rule.p_cu_limit.map(|v| format_compact(v as f64, 0)).unwrap_or_else(|| "—".into()) }</td>
                <td class="dim-col">{ rule.p_cu_price.map(|v| format_compact(v as f64, 0)).unwrap_or_else(|| "—".into()) }</td>
                <td class="labels-col">{ labels_display }</td>
                <td class="num-col">{ format!("{:.3}", rule.buy_amount) }</td>
                <td class="tp-col">{ format!("{:.1}%", rule.take_profit) }</td>
                <td class="sl-col">{ format!("{:.1}%", rule.stop_loss) }</td>
                <td class="status-col">
                    <button
                        class={if rule.is_active { "status-pill status-active" } else { "status-pill status-inactive" }}
                        onclick={on_toggle_cb}
                        title="Toggle active/inactive"
                    >
                        { if rule.is_active { "● Active" } else { "○ Inactive" } }
                    </button>
                </td>
                <td class="actions-col">
                    if is_confirming {
                        <span class="confirm-text">{ "Delete?" }</span>
                        <button class="act-btn act-danger" onclick={on_confirm_delete.clone()} disabled={*delete_loading}>{ "Yes" }</button>
                        <button class="act-btn" onclick={on_cancel_delete.clone()}>{ "No" }</button>
                    } else {
                        <button class="act-btn act-edit" onclick={on_edit_cb} title="Edit rule">{ "Edit" }</button>
                        <button class="act-btn act-danger" onclick={on_delete_cb} title="Delete rule">{ "Del" }</button>
                        <button class="act-btn act-sim" onclick={on_sim_cb} disabled={*simulate_loading} title="Run simulation">{ "▶" }</button>
                    }
                </td>
            </tr>
        }
    }).collect::<Html>();

    // ── Simulation summary card (shown above rules table) ─────────────────────
    let sim_summary_card = if let Some(result) = &*simulate_result {
        let clear_sim_top = {
            let simulate_result = simulate_result.clone();
            let simulate_error  = simulate_error.clone();
            Callback::from(move |_: MouseEvent| {
                simulate_result.set(None);
                simulate_error.set(None);
            })
        };
        html! {
            <div class="sim-summary-card">
                <div class="sim-summary-header">
                    <span class="sim-summary-title">
                        { "Simulation — " }{ &result.rule_name }
                    </span>
                    <button class="sim-close-btn" onclick={clear_sim_top} title="Close">{ "✕" }</button>
                </div>
                <div class="sim-summary-grid">
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Tokens" }</div>
                        <div class="sim-summary-value">{ result.tokens_matched }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Win Rate" }</div>
                        <div class={if result.win_rate_pct >= 50.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>
                            { format!("{:.1}%", result.win_rate_pct) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "W / L / Open" }</div>
                        <div class="sim-summary-value">
                            <span class="tp-col">{ result.win_count }</span>
                            { " / " }
                            <span class="sl-col">{ result.loss_count }</span>
                            { " / " }
                            <span class="dim-col">{ result.open_count }</span>
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Total PnL" }</div>
                        <div class={if result.total_pnl_sol >= 0.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>
                            { format!("{:+.4} SOL", result.total_pnl_sol) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg PnL" }</div>
                        <div class={
                            match result.avg_pnl_pct {
                                Some(v) if v >= 0.0 => "sim-summary-value sv-primary",
                                Some(_) => "sim-summary-value sv-danger",
                                None => "sim-summary-value",
                            }
                        }>
                            { result.avg_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg Hold" }</div>
                        <div class="sim-summary-value">
                            { result.avg_holding_secs.map(|s| format_age(s as i64)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Best" }</div>
                        <div class="sim-summary-value tp-col">
                            { result.best_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Worst" }</div>
                        <div class="sim-summary-value sl-col">
                            { result.worst_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                </div>
            </div>
        }
    } else {
        html! {}
    };

    // ── Simulation tokens panel (shown below rules table) ─────────────────────
    let sim_panel = if *simulate_loading {
        html! {
            <div class="sim-loading">
                <span class="sim-spinner">{ "⟳" }</span>
                { " Running simulation…" }
            </div>
        }
    } else if let Some(err) = &*simulate_error {
        html! { <div class="inline-error">{ err }</div> }
    } else if let Some(result) = &*simulate_result {
        let token_rows = result.tokens.iter().enumerate().map(|(i, t)| {
            let entry_time_str = t.entry_time.get(..16).unwrap_or(&t.entry_time).replace('T', " ");
            let exit_time_str = t.exit_time.as_deref()
                .map(|s| s.get(..16).unwrap_or(s).replace('T', " "))
                .unwrap_or_else(|| "—".into());
            let hold_str = t.holding_secs.map(format_age).unwrap_or_else(|| "—".into());
            let pnl_pct_html = match t.pnl_percent {
                Some(v) if v >= 0.0 => html! { <span class="tp-col">{ format!("{:+.1}%", v) }</span> },
                Some(v)             => html! { <span class="sl-col">{ format!("{:.1}%", v)  }</span> },
                None                => html! { <span class="dim-col">{ "—" }</span> },
            };
            let pnl_sol_html = match t.pnl_sol {
                Some(v) if v >= 0.0 => html! { <span class="tp-col">{ format!("{:+.4}", v) }</span> },
                Some(v)             => html! { <span class="sl-col">{ format!("{:.4}", v)  }</span> },
                None                => html! { <span class="dim-col">{ "—" }</span> },
            };
            let exit_reason_html = match t.exit_reason.as_str() {
                "TakeProfit" => html! { <span class="tp-col">{ "TP" }</span> },
                "StopLoss"   => html! { <span class="sl-col">{ "SL" }</span> },
                _            => html! { <span class="dim-col">{ "Open" }</span> },
            };
            html! {
                <tr key={t.mint.clone()}>
                    <td class="row-num">{ i + 1 }</td>
                    <td>
                        <a href={format!("https://gmgn.ai/sol/token/{}", t.mint)}
                            target="_blank" rel="noreferrer" class="symbol-link-inline">
                            { &t.symbol }
                        </a>
                    </td>
                    <td class="num-col">{ format_price(t.entry_price) }</td>
                    <td class="dim-col">{ entry_time_str }</td>
                    <td class="num-col">{ t.exit_price.map(format_price).unwrap_or_else(|| "—".into()) }</td>
                    <td class="dim-col">{ exit_time_str }</td>
                    <td class="dim-col">{ hold_str }</td>
                    <td>{ pnl_pct_html }</td>
                    <td>{ pnl_sol_html }</td>
                    <td>{ exit_reason_html }</td>
                    <td class="dim-col">{ t.total_trades }</td>
                </tr>
            }
        }).collect::<Html>();

        html! {
            <div class="sim-result">
                <div class="sim-result-header">
                    <span class="sim-result-title">
                        { "Simulated Tokens — " }{ &result.rule_name }
                        <span class="matched-count-badge">{ result.tokens_matched }</span>
                    </span>
                </div>
                if result.tokens.is_empty() {
                    <div class="matched-empty">{ "No tokens matched this rule's entry criteria." }</div>
                } else {
                    <div class="table-wrapper" style="margin: 0;">
                        <div class="table-scroll">
                            <table class="trade-table">
                                <thead>
                                    <tr>
                                        <th class="th-num">{ "#" }</th>
                                        <th>{ "Symbol" }</th>
                                        <th>{ "Entry Price" }</th>
                                        <th>{ "Entry Time" }</th>
                                        <th>{ "Exit Price" }</th>
                                        <th>{ "Exit Time" }</th>
                                        <th>{ "Holding" }</th>
                                        <th>{ "PnL%" }</th>
                                        <th>{ "PnL SOL" }</th>
                                        <th>{ "Reason" }</th>
                                        <th>{ "Trades" }</th>
                                    </tr>
                                </thead>
                                <tbody>{ token_rows }</tbody>
                            </table>
                        </div>
                    </div>
                }
            </div>
        }
    } else {
        html! {}
    };
    let is_edit = matches!(&*modal_mode, ModalMode::Edit(_));
    let modal_title = if is_edit { "Edit TPSL Rule" } else { "New TPSL Rule" };
    let modal_visible = !matches!(&*modal_mode, ModalMode::None);

    macro_rules! oninput {
        ($field:expr) => {{
            let field = $field.clone();
            Callback::from(move |e: InputEvent| {
                let el: web_sys::HtmlInputElement = e.target_unchecked_into();
                field.set(el.value());
            })
        }};
    }

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">

                // ── Simulation summary card (above rules table) ───────────────
                { sim_summary_card }

                // ── Header bar ────────────────────────────────────────────────
                <div class="strat-header">
                    <div class="strat-title-row">
                        <h2 class="section-title">{ "TPSL Strategies" }</h2>
                        <span class="token-count-badge">{ filtered.len() }{ " rules" }</span>
                    </div>
                    <button class="add-rule-btn" onclick={open_add}>{ "+ Add Rule" }</button>
                </div>

                // ── Search bar ────────────────────────────────────────────────
                <div class="strat-toolbar">
                    <input
                        type="search"
                        class="tokens-search"
                        placeholder="Search rules by name…"
                        value={(*search).clone()}
                        oninput={Callback::from({
                            let search = search.clone();
                            move |e: InputEvent| {
                                let el: web_sys::HtmlInputElement = e.target_unchecked_into();
                                search.set(el.value());
                            }
                        })}
                    />
                </div>

                // ── Rules table ───────────────────────────────────────────────
                if *loading {
                    <div class="strat-state-msg">{ "Loading rules…" }</div>
                } else if let Some(err) = &*load_error {
                    <div class="inline-error">{ err }</div>
                } else {
                    <div class="table-wrapper">
                        <div class="table-scroll">
                            <table class="trade-table">
                                <thead>
                                    <tr>
                                        <th>{ "Name" }</th>
                                        <th>{ "Init Buy" }</th>
                                        <th>{ "CU Lim" }</th>
                                        <th>{ "CU Price" }</th>
                                        <th>{ "Labels" }</th>
                                        <th>{ "Buy Amt" }</th>
                                        <th>{ "TP" }</th>
                                        <th>{ "SL" }</th>
                                        <th>{ "Status" }</th>
                                        <th>{ "Actions" }</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    if filtered.is_empty() {
                                        <tr><td colspan="10" class="no-data">{ "No rules found" }</td></tr>
                                    } else {
                                        { rule_rows }
                                    }
                                </tbody>
                            </table>
                        </div>
                    </div>
                }

                // ── Simulation tokens panel ───────────────────────────────────
                { sim_panel }

                // ── Add / Edit modal ──────────────────────────────────────────
                <Modal title={modal_title.to_string()} visible={modal_visible} on_close={close_modal}>
                    <div class="rule-form">
                        <div class="rule-form-grid">

                            <label class="form-field form-field-full">
                                <span class="form-label">{ "Rule Name" }</span>
                                <input type="text" class="form-input" value={(*f_name).clone()}
                                    oninput={oninput!(f_name)} placeholder="e.g. Sniper 0.5 SOL" />
                            </label>

                            <label class="form-field">
                                <span class="form-label">{ "Initial Buy SOL" }</span>
                                <input type="number" step="0.001"
                                    class={if is_edit { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_initial_buy).clone()} oninput={oninput!(f_initial_buy)}
                                    placeholder="0.5" readonly={is_edit} />
                            </label>

                            <label class="form-field">
                                <span class="form-label">
                                    { "CU Limit" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number"
                                    class={if is_edit { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_limit).clone()} oninput={oninput!(f_cu_limit)}
                                    placeholder="e.g. 200000" readonly={is_edit} />
                            </label>

                            <label class="form-field">
                                <span class="form-label">
                                    { "CU Price" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number"
                                    class={if is_edit { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_price).clone()} oninput={oninput!(f_cu_price)}
                                    placeholder="e.g. 1000000" readonly={is_edit} />
                            </label>

                            <label class="form-field form-field-full">
                                <span class="form-label">
                                    { "Instruction Labels" }
                                    <span class="form-opt">{ " comma-separated, opt" }</span>
                                </span>
                                <input type="text"
                                    class={if is_edit { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_ix_labels).clone()} oninput={oninput!(f_ix_labels)}
                                    placeholder="label1, label2" readonly={is_edit} />
                            </label>

                            <label class="form-field">
                                <span class="form-label">{ "Buy Amount (SOL)" }</span>
                                <input type="number" step="0.001" class="form-input"
                                    value={(*f_buy_amount).clone()} oninput={oninput!(f_buy_amount)} placeholder="0.1" />
                            </label>

                            <label class="form-field">
                                <span class="form-label">{ "Take Profit %" }</span>
                                <input type="number" step="1" class="form-input form-input-tp"
                                    value={(*f_take_profit).clone()} oninput={oninput!(f_take_profit)} placeholder="50" />
                            </label>

                            <label class="form-field">
                                <span class="form-label">{ "Stop Loss %" }</span>
                                <input type="number" step="1" class="form-input form-input-sl"
                                    value={(*f_stop_loss).clone()} oninput={oninput!(f_stop_loss)} placeholder="20" />
                            </label>

                        </div>

                        if is_edit {
                            <p class="form-hint">
                                { "Entry criteria (Init Buy, CU Limit, CU Price, Labels) are locked after creation." }
                            </p>
                        }

                        { if let Some(err) = &*form_error {
                            html! { <div class="inline-error form-error">{ err }</div> }
                        } else { html! {} }}

                        <div class="form-actions">
                            <button class="btn-ghost" onclick={cancel_modal}>{ "Cancel" }</button>
                            <button class="btn-primary-sm" onclick={on_save} disabled={*form_loading}>
                                { if *form_loading { "Saving…" } else { "Save Rule" } }
                            </button>
                        </div>
                    </div>
                </Modal>

            </main>
        </div>
    }
}


