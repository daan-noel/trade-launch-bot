// Utility: display '-' for None or zero/empty values
fn dash<T: PartialEq + Default + ToString>(val: Option<T>) -> String {
    match val {
        Some(v) if v != T::default() => v.to_string(),
        _ => "-".to_string(),
    }
}
fn dash_f(val: f64, precision: usize) -> String {
    if val == 0.0 {
        "-".to_string()
    } else {
        format_decimal_trim(val, precision)
    }
}
fn dash_percent(val: f64) -> String {
    if val == 0.0 {
        "-".to_string()
    } else {
        format!("{}%", format_decimal_trim(val, 1))
    }
}
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::modal::Modal;
use crate::components::Header;
use crate::services::api::{
    create_tpsl_rule, delete_tpsl_rule, fetch_tpsl_rules, simulate_tpsl_rule, update_tpsl_rule,
    CreateRuleRequest, RuleRecord, SimulatedTokenResult, UpdateRuleRequest,
};
use crate::state::PriceUnitContext;
use crate::utils::format::{format_age, format_compact, format_decimal_trim};

// ── Modal mode ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum ModalMode {
    None,
    Add,
    Edit(RuleRecord),
}

// ── Simulation result wrapper ──────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
struct SimulationResult {
    rule_name: String,
    tokens: Vec<SimulatedTokenResult>,
}

// ── Page ──────────────────────────────────────────────────────────────────────

#[function_component(StrategyPage)]
pub fn strategy_page() -> Html {
    // ── Data ──────────────────────────────────────────────────────────────────
    let rules = use_state(Vec::<RuleRecord>::new);
    let loading = use_state(|| false);
    let load_error = use_state(|| Option::<String>::None);
    let search = use_state(String::new);

    // ── Selection state ───────────────────────────────────────────────────────
    let selected_rule_id = use_state(|| Option::<String>::None);

    // ── Rule positions state ──────────────────────────────────────────────────
    use crate::services::api::fetch_rule_positions;
    use crate::services::api::RulePositionRecord;
    let rule_positions = use_state(Vec::<RulePositionRecord>::new);
    let rule_positions_loading = use_state(|| false);
    let rule_positions_error = use_state(|| Option::<String>::None);

    // Fetch positions when selected rule changes and is active
    {
        let selected_rule_id = selected_rule_id.clone();
        let rules = rules.clone();
        let rule_positions = rule_positions.clone();
        let rule_positions_loading = rule_positions_loading.clone();
        let rule_positions_error = rule_positions_error.clone();
        use_effect_with(selected_rule_id.clone(), move |selected| {
            let selected = selected.clone();
            let rules = rules.clone();
            let rule_positions = rule_positions.clone();
            let rule_positions_loading = rule_positions_loading.clone();
            let rule_positions_error = rule_positions_error.clone();
            if let Some(rule_id) = selected.as_ref() {
                if let Some(rule) = (*rules).iter().find(|r| &r.id == rule_id) {
                    if rule.is_active {
                        rule_positions_loading.set(true);
                        rule_positions_error.set(None);
                        let rule_id = rule_id.clone();
                        spawn_local(async move {
                            match fetch_rule_positions(&rule_id).await {
                                Ok(positions) => rule_positions.set(positions),
                                Err(err) => rule_positions_error.set(Some(err)),
                            }
                            rule_positions_loading.set(false);
                        });
                    } else {
                        rule_positions.set(vec![]);
                        rule_positions_error.set(None);
                        rule_positions_loading.set(false);
                    }
                }
            } else {
                rule_positions.set(vec![]);
                rule_positions_error.set(None);
                rule_positions_loading.set(false);
            }
            || ()
        });
    }

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
    let f_max_sol_cost = use_state(String::new);
    let f_spendable_sol_in = use_state(String::new);
    let f_max_holding_tokens = use_state(String::new);
    let f_total_max_trade_tokens = use_state(String::new);
    let f_tolerance = use_state(String::new);
    let f_allow_edit_params = use_state(|| false);
    let form_error = use_state(|| Option::<String>::None);
    let form_loading = use_state(|| false);

    // ── Delete confirm ────────────────────────────────────────────────────────
    let confirm_delete_id = use_state(|| Option::<String>::None);
    let delete_loading = use_state(|| false);

    // ── Simulation ────────────────────────────────────────────────────────────
    let simulate_result = use_state(|| Option::<SimulationResult>::None);
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
            modal_mode.clone(),
            f_name.clone(),
            f_initial_buy.clone(),
            f_cu_limit.clone(),
            f_cu_price.clone(),
        );
        let (
            f_ix_labels,
            f_buy_amount,
            f_take_profit,
            f_stop_loss,
            f_max_sol_cost,
            f_spendable_sol_in,
            f_max_holding_tokens,
            f_total_max_trade_tokens,
            f_tolerance,
            f_allow_edit_params,
            form_error,
        ) = (
            f_ix_labels.clone(),
            f_buy_amount.clone(),
            f_take_profit.clone(),
            f_stop_loss.clone(),
            f_max_sol_cost.clone(),
            f_spendable_sol_in.clone(),
            f_max_holding_tokens.clone(),
            f_total_max_trade_tokens.clone(),
            f_tolerance.clone(),
            f_allow_edit_params.clone(),
            form_error.clone(),
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
            f_max_sol_cost.set(String::new());
            f_spendable_sol_in.set(String::new());
            f_max_holding_tokens.set(String::new());
            f_total_max_trade_tokens.set(String::new());
            f_tolerance.set("0".into());
            f_allow_edit_params.set(false);
            form_error.set(None);
            modal_mode.set(ModalMode::Add);
        })
    };

    let populate_example_labels = {
        let f_ix_labels = f_ix_labels.clone();
        Callback::from(move |_: MouseEvent| {
            f_ix_labels.set(r#"["Compute Budget: SetComputeUnitLimit", "Compute Budget: SetComputeUnitPrice", "Pump.Fun: Create_v2", "Associated Token: CreateIdempotent", "Pump.Fun: Buy", "System Program: Transfer"]"#.into());
        })
    };

    let open_edit = {
        let (modal_mode, f_name, f_initial_buy, f_cu_limit, f_cu_price) = (
            modal_mode.clone(),
            f_name.clone(),
            f_initial_buy.clone(),
            f_cu_limit.clone(),
            f_cu_price.clone(),
        );
        let (
            f_ix_labels,
            f_buy_amount,
            f_take_profit,
            f_stop_loss,
            f_max_sol_cost,
            f_spendable_sol_in,
            f_max_holding_tokens,
            f_total_max_trade_tokens,
            f_tolerance,
            f_allow_edit_params,
            form_error,
        ) = (
            f_ix_labels.clone(),
            f_buy_amount.clone(),
            f_take_profit.clone(),
            f_stop_loss.clone(),
            f_max_sol_cost.clone(),
            f_spendable_sol_in.clone(),
            f_max_holding_tokens.clone(),
            f_total_max_trade_tokens.clone(),
            f_tolerance.clone(),
            f_allow_edit_params.clone(),
            form_error.clone(),
        );
        Callback::from(move |rule: RuleRecord| {
            f_name.set(rule.rule_name.clone());
            f_initial_buy.set(
                rule.p_initial_buy_sol
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            f_cu_limit.set(rule.p_cu_limit.map(|v| v.to_string()).unwrap_or_default());
            f_cu_price.set(rule.p_cu_price.map(|v| v.to_string()).unwrap_or_default());
            f_max_sol_cost.set(
                rule.p_max_sol_cost
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            f_spendable_sol_in.set(
                rule.p_spendable_sol_in
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            f_max_holding_tokens.set(
                rule.p_max_holding_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            f_total_max_trade_tokens.set(
                rule.p_total_max_trade_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            f_tolerance.set(rule.tolerance_pct.to_string());

            let labels = if rule.p_ix_labels.is_array() {
                serde_json::to_string(&rule.p_ix_labels).unwrap_or_default()
            } else {
                String::new()
            };

            f_ix_labels.set(labels);
            f_buy_amount.set(rule.buy_amount.to_string());
            f_take_profit.set(rule.take_profit.to_string());
            f_stop_loss.set(rule.stop_loss.to_string());
            f_allow_edit_params.set(false);
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

    let toggle_edit_params = {
        let f_allow_edit_params = f_allow_edit_params.clone();
        Callback::from(move |_: MouseEvent| f_allow_edit_params.set(!*f_allow_edit_params))
    };

    // ── Save (create or update) ───────────────────────────────────────────────
    let on_save = {
        let (modal_mode, rules, form_error, form_loading) = (
            modal_mode.clone(),
            rules.clone(),
            form_error.clone(),
            form_loading.clone(),
        );
        let (
            f_name,
            f_initial_buy,
            f_cu_limit,
            f_cu_price,
            f_ix_labels,
            f_max_sol_cost,
            f_spendable_sol_in,
            f_max_holding_tokens,
            f_total_max_trade_tokens,
            f_tolerance,
        ) = (
            f_name.clone(),
            f_initial_buy.clone(),
            f_cu_limit.clone(),
            f_cu_price.clone(),
            f_ix_labels.clone(),
            f_max_sol_cost.clone(),
            f_spendable_sol_in.clone(),
            f_max_holding_tokens.clone(),
            f_total_max_trade_tokens.clone(),
            f_tolerance.clone(),
        );
        let (f_buy_amount, f_take_profit, f_stop_loss) = (
            f_buy_amount.clone(),
            f_take_profit.clone(),
            f_stop_loss.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let mode = (*modal_mode).clone();
            let (rules, form_error, form_loading, modal_mode) = (
                rules.clone(),
                form_error.clone(),
                form_loading.clone(),
                modal_mode.clone(),
            );
            let (
                name,
                initial_buy_s,
                cu_limit_s,
                cu_price_s,
                ix_labels_s,
                max_sol_cost_s,
                spendable_sol_in_s,
                max_holding_tokens_s,
                total_max_trade_tokens_s,
                tolerance_s,
            ) = (
                (*f_name).clone(),
                (*f_initial_buy).clone(),
                (*f_cu_limit).clone(),
                (*f_cu_price).clone(),
                (*f_ix_labels).clone(),
                (*f_max_sol_cost).clone(),
                (*f_spendable_sol_in).clone(),
                (*f_max_holding_tokens).clone(),
                (*f_total_max_trade_tokens).clone(),
                (*f_tolerance).clone(),
            );
            let (buy_amount_s, take_profit_s, stop_loss_s) = (
                (*f_buy_amount).clone(),
                (*f_take_profit).clone(),
                (*f_stop_loss).clone(),
            );
            form_error.set(None);
            form_loading.set(true);

            spawn_local(async move {
                let buy_amount = match buy_amount_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => {
                        form_error.set(Some("Invalid buy amount".into()));
                        form_loading.set(false);
                        return;
                    }
                };
                let take_profit = match take_profit_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => {
                        form_error.set(Some("Invalid take profit %".into()));
                        form_loading.set(false);
                        return;
                    }
                };
                let stop_loss = match stop_loss_s.trim().parse::<f64>() {
                    Ok(v) => v,
                    Err(_) => {
                        form_error.set(Some("Invalid stop loss %".into()));
                        form_loading.set(false);
                        return;
                    }
                };

                match mode {
                    ModalMode::Add => {
                        let p_initial_buy_sol = if initial_buy_s.trim().is_empty() {
                            None
                        } else {
                            match initial_buy_s.trim().parse::<f64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid initial buy SOL".into()));
                                    form_loading.set(false);
                                    return;
                                }
                            }
                        };
                        let p_cu_limit = if cu_limit_s.trim().is_empty() {
                            None
                        } else {
                            cu_limit_s.trim().parse::<u64>().ok()
                        };
                        let p_cu_price = if cu_price_s.trim().is_empty() {
                            None
                        } else {
                            cu_price_s.trim().parse::<u64>().ok()
                        };
                        let ix_labels: Vec<Value> = if ix_labels_s.trim().starts_with('[') {
                            match serde_json::from_str::<Value>(ix_labels_s.trim()) {
                                Ok(Value::Array(arr)) => arr
                                    .into_iter()
                                    .map(|item| {
                                        if item.is_string() {
                                            item
                                        } else {
                                            Value::String(item.to_string())
                                        }
                                    })
                                    .collect(),
                                _ => ix_labels_s
                                    .split(',')
                                    .map(|s| s.trim())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| Value::String(s.to_string()))
                                    .collect(),
                            }
                        } else {
                            ix_labels_s
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(|s| Value::String(s.to_string()))
                                .collect()
                        };

                        let p_max_sol_cost = if max_sol_cost_s.trim().is_empty() {
                            None
                        } else {
                            max_sol_cost_s.trim().parse::<f64>().ok()
                        };
                        let p_spendable_sol_in = if spendable_sol_in_s.trim().is_empty() {
                            None
                        } else {
                            spendable_sol_in_s.trim().parse::<f64>().ok()
                        };
                        let p_max_holding_tokens = if max_holding_tokens_s.trim().is_empty() {
                            None
                        } else {
                            match max_holding_tokens_s.trim().parse::<u64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid max holding tokens".into()));
                                    form_loading.set(false);
                                    return;
                                }
                            }
                        };
                        let p_total_max_trade_tokens = if total_max_trade_tokens_s.trim().is_empty() {
                            None
                        } else {
                            match total_max_trade_tokens_s.trim().parse::<u64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid total max trade tokens".into()));
                                    form_loading.set(false);
                                    return;
                                }
                            }
                        };
                        let p_tolerance = if tolerance_s.trim().is_empty() {
                            None
                        } else {
                            match tolerance_s.trim().parse::<f64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid tolerance %".into()));
                                    form_loading.set(false);
                                    return;
                                }
                            }
                        };
                        let req = CreateRuleRequest {
                            rule_name: name,
                            p_initial_buy_sol,
                            p_cu_limit,
                            p_cu_price,
                            p_max_sol_cost,
                            p_spendable_sol_in,
                            p_max_holding_tokens,
                            p_total_max_trade_tokens,
                            p_ix_labels: Value::Array(ix_labels),
                            buy_amount,
                            take_profit,
                            stop_loss,
                            tolerance_pct: p_tolerance,
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
                        let p_initial_buy_sol = Some(Some(if initial_buy_s.trim().is_empty() { 0.0 } else { initial_buy_s.trim().parse::<f64>().unwrap_or(0.0) }));
                        let p_cu_limit = Some(Some(if cu_limit_s.trim().is_empty() { 0 } else { cu_limit_s.trim().parse::<u64>().unwrap_or(0) }));
                        let p_cu_price = Some(Some(if cu_price_s.trim().is_empty() { 0 } else { cu_price_s.trim().parse::<u64>().unwrap_or(0) }));
                        let p_ix_labels = if ix_labels_s.trim().is_empty() {
                            Some(Some(Value::Array(vec![])))
                        } else {
                            let labels_vec: Vec<Value> = if ix_labels_s.trim().starts_with('[') {
                                match serde_json::from_str::<Value>(ix_labels_s.trim()) {
                                    Ok(Value::Array(arr)) => arr
                                        .into_iter()
                                        .map(|item| {
                                            if item.is_string() {
                                                item
                                            } else {
                                                Value::String(item.to_string())
                                            }
                                        })
                                        .collect(),
                                    _ => ix_labels_s
                                        .split(',')
                                        .map(|s| s.trim())
                                        .filter(|s| !s.is_empty())
                                        .map(|s| Value::String(s.to_string()))
                                        .collect(),
                                }
                            } else {
                                ix_labels_s
                                    .split(',')
                                    .map(|s| s.trim())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| Value::String(s.to_string()))
                                    .collect()
                            };
                            Some(Some(Value::Array(labels_vec)))
                        };
                        let p_max_sol_cost = Some(Some(if max_sol_cost_s.trim().is_empty() { 0.0 } else { max_sol_cost_s.trim().parse::<f64>().unwrap_or(0.0) }));
                        let p_spendable_sol_in = Some(Some(if spendable_sol_in_s.trim().is_empty() { 0.0 } else { spendable_sol_in_s.trim().parse::<f64>().unwrap_or(0.0) }));
                        let p_max_holding_tokens = Some(Some(if max_holding_tokens_s.trim().is_empty() { 0 } else { max_holding_tokens_s.trim().parse::<u64>().unwrap_or(0) }));
                        let p_total_max_trade_tokens = Some(Some(if total_max_trade_tokens_s.trim().is_empty() { 0 } else { total_max_trade_tokens_s.trim().parse::<u64>().unwrap_or(0) }));
                        let p_tolerance = if tolerance_s.trim().is_empty() {
                            None
                        } else {
                            match tolerance_s.trim().parse::<f64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid tolerance %".into()));
                                    form_loading.set(false);
                                    return;
                                }
                            }
                        };
                        let req = UpdateRuleRequest {
                            rule_name: Some(name),
                            buy_amount: Some(buy_amount),
                            take_profit: Some(take_profit),
                            stop_loss: Some(stop_loss),
                            p_initial_buy_sol,
                            p_cu_limit,
                            p_cu_price,
                            p_ix_labels,
                            p_max_sol_cost,
                            p_spendable_sol_in,
                            p_max_holding_tokens,
                            p_total_max_trade_tokens,
                            tolerance_pct: p_tolerance,
                            is_active: None,
                        };
                        match update_tpsl_rule(&rule.id, &req).await {
                            Ok(updated) => {
                                let items = (*rules)
                                    .iter()
                                    .map(|r| {
                                        if r.id == updated.id {
                                            updated.clone()
                                        } else {
                                            r.clone()
                                        }
                                    })
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
                    rule_name: None,
                    buy_amount: None,
                    take_profit: None,
                    stop_loss: None,
                    p_initial_buy_sol: None,
                    p_cu_limit: None,
                    p_cu_price: None,
                    p_ix_labels: None,
                    p_max_sol_cost: None,
                    p_spendable_sol_in: None,
                    p_max_holding_tokens: None,
                    p_total_max_trade_tokens: None,
                    tolerance_pct: None,
                    is_active: Some(!rule.is_active),
                };
                if let Ok(updated) = update_tpsl_rule(&rule.id, &req).await {
                    let items = (*rules)
                        .iter()
                        .map(|r| {
                            if r.id == updated.id {
                                updated.clone()
                            } else {
                                r.clone()
                            }
                        })
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
            confirm_delete_id.clone(),
            rules.clone(),
            delete_loading.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let rule_id = match (*confirm_delete_id).clone() {
                Some(id) => id,
                None => return,
            };
            let (confirm_delete_id, rules, delete_loading) = (
                confirm_delete_id.clone(),
                rules.clone(),
                delete_loading.clone(),
            );
            delete_loading.set(true);
            spawn_local(async move {
                if delete_tpsl_rule(&rule_id).await.is_ok() {
                    let items = (*rules)
                        .iter()
                        .filter(|r| r.id != rule_id)
                        .cloned()
                        .collect();
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
            simulate_result.clone(),
            simulate_error.clone(),
            simulate_loading.clone(),
        );
        Callback::from(move |rule: RuleRecord| {
            let (simulate_result, simulate_error, simulate_loading) = (
                simulate_result.clone(),
                simulate_error.clone(),
                simulate_loading.clone(),
            );
            simulate_result.set(None);
            simulate_error.set(None);
            simulate_loading.set(true);
            let rule_name = rule.rule_name.clone();
            spawn_local(async move {
                match simulate_tpsl_rule(&rule.id).await {
                    Ok(tokens) => simulate_result.set(Some(SimulationResult { rule_name, tokens })),
                    Err(err) => simulate_error.set(Some(err)),
                }
                simulate_loading.set(false);
            });
        })
    };

    let search_val = (*search).to_lowercase();
    let filtered: Vec<&RuleRecord> = (*rules)
        .iter()
        .filter(|r| search_val.is_empty() || r.rule_name.to_lowercase().contains(&search_val))
        .collect();

    // ── Build table rows ──────────────────────────────────────────────────────
    let on_select_rule = {
        let selected_rule_id = selected_rule_id.clone();
        Callback::from(move |rule_id: String| selected_rule_id.set(Some(rule_id)))
    };

    let rule_rows = filtered.iter().map(|rule| {
        let rule = (*rule).clone();
        let is_selected = Some(rule.id.clone()) == *selected_rule_id;

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
            let rule = rule.clone();
            Callback::from(move |_: MouseEvent| on_simulate.emit(rule.clone()))
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
            .unwrap_or_else(|| "-".to_string());
        let ix_count = rule.p_ix_labels.as_array().map(|arr| arr.len()).unwrap_or(0);

        html! {
            <tr key={rule.id.clone()}
                class={if is_selected { "selected-row" } else { "" }}
                onclick={
                    let rule_id = rule.id.clone();
                    let on_select_rule = on_select_rule.clone();
                    Callback::from(move |_: MouseEvent| on_select_rule.emit(rule_id.clone()))
                }
                style="cursor:pointer;"
            >
                <td>
                    <span class="rule-name-cell">{ &rule.rule_name }</span>
                </td>
                <td class="num-col">{ dash_f(rule.p_initial_buy_sol.unwrap_or(0.0), 15) }</td>
                <td class="dim-col">{ dash(rule.p_cu_limit) }</td>
                <td class="dim-col">{ dash(rule.p_cu_price) }</td>
                <td class="num-col">{ dash_f(rule.p_max_sol_cost.unwrap_or(0.0), 3) }</td>
                <td class="num-col">{ dash_f(rule.p_spendable_sol_in.unwrap_or(0.0), 3) }</td>
                <td class="num-col">{ dash(rule.p_max_holding_tokens) }</td>
                <td class="num-col">{ dash(rule.p_total_max_trade_tokens) }</td>
                <td class="num-col">{ if ix_count > 0 { ix_count.to_string() } else { "-".into() } }</td>
                <td class="labels-col">{ labels_display }</td>
                <td class="num-col">{ dash_f(rule.buy_amount, 15) }</td>
                <td class="tp-col">{ dash_percent(rule.take_profit) }</td>
                <td class="sl-col">{ dash_percent(rule.stop_loss) }</td>
                <td class="num-col">{ dash_percent(rule.tolerance_pct) }</td>
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
                        <button class="act-btn act-edit" onclick={on_edit_cb} disabled={rule.is_active} title={if rule.is_active { "Cannot edit active rules" } else { "Edit rule" }}>{ "Edit" }</button>
                        <button class="act-btn act-danger" onclick={on_delete_cb} disabled={rule.is_active} title={if rule.is_active { "Cannot delete active rules" } else { "Delete rule" }}>{ "Del" }</button>
                        <button class="act-btn act-sim" onclick={on_sim_cb} disabled={*simulate_loading} title="Run simulation">{ "▶" }</button>
                    }
                </td>
            </tr>
        }
    }).collect::<Html>();

    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above StrategyPage");

    // ── Simulation summary card (shown above rules table) ─────────────────────
    let sim_summary_card = if let Some(result) = &*simulate_result {
        let clear_sim_top = {
            let simulate_result = simulate_result.clone();
            let simulate_error = simulate_error.clone();
            Callback::from(move |_: MouseEvent| {
                simulate_result.set(None);
                simulate_error.set(None);
            })
        };

        let tokens = &result.tokens;
        let tokens_matched = tokens.len();
        let win_count = tokens
            .iter()
            .filter(|t| t.exit_reason == "TakeProfit")
            .count();
        let loss_count = tokens
            .iter()
            .filter(|t| t.exit_reason == "StopLoss")
            .count();
        let open_count = tokens.iter().filter(|t| t.exit_reason == "Open").count();
        let closed_count = tokens_matched - open_count;
        let win_rate_pct = if closed_count > 0 {
            (win_count as f64 / closed_count as f64) * 100.0
        } else {
            0.0
        };

        let total_entry_amount: f64 = tokens.iter().map(|t| t.entry_amount).sum();
        let total_holding_amount: f64 = tokens
            .iter()
            .filter(|t| t.exit_reason == "Open")
            .map(|t| t.entry_amount)
            .sum();
        let total_tp_amount: f64 = tokens
            .iter()
            .filter(|t| t.exit_reason == "TakeProfit")
            .filter_map(|t| t.pnl_sol)
            .sum();
        let total_sl_amount: f64 = tokens
            .iter()
            .filter(|t| t.exit_reason == "StopLoss")
            .filter_map(|t| t.pnl_sol.map(|v| v.abs()))
            .sum();
        let total_pnl_sol = total_tp_amount - total_sl_amount;

        let closed: Vec<&SimulatedTokenResult> =
            tokens.iter().filter(|t| t.exit_reason != "Open").collect();
        let avg_pnl_pct = if !closed.is_empty() {
            Some(closed.iter().filter_map(|t| t.pnl_percent).sum::<f64>() / closed.len() as f64)
        } else {
            None
        };
        let avg_entry_amount = if tokens_matched > 0 {
            Some(total_entry_amount / tokens_matched as f64)
        } else {
            None
        };
        let avg_holding_secs = if !closed.is_empty() {
            Some(
                closed
                    .iter()
                    .filter_map(|t| t.holding_secs)
                    .map(|s| s as f64)
                    .sum::<f64>()
                    / closed.len() as f64,
            )
        } else {
            None
        };
        let best_pnl_pct = closed.iter().filter_map(|t| t.pnl_percent).reduce(f64::max);
        let worst_pnl_pct = closed.iter().filter_map(|t| t.pnl_percent).reduce(f64::min);

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
                        <div class="sim-summary-value">{ tokens_matched }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Win Rate" }</div>
                        <div class={if win_rate_pct >= 50.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>
                            { format!("{:.1}%", win_rate_pct) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "W / L / Open" }</div>
                        <div class="sim-summary-value">
                            <span class="tp-col">{ win_count }</span>
                            { " / " }
                            <span class="sl-col">{ loss_count }</span>
                            { " / " }
                            <span class="dim-col">{ open_count }</span>
                        </div>
                    </div>

                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total Entry ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value">
                            { price_unit.display_amount(total_entry_amount) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total Holding ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value">
                            { price_unit.display_amount(total_holding_amount) }
                        </div>
                    </div>
                                        <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg Entry" }</div>
                        <div class="sim-summary-value">
                            { avg_entry_amount.map(|v| price_unit.display_amount(v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>

                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total PnL ({})", price_unit.unit_label()) }</div>
                        <div class={if total_pnl_sol >= 0.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>
                            { price_unit.display_amount(total_pnl_sol) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg PnL" }</div>
                        <div class={
                            match avg_pnl_pct {
                                Some(v) if v >= 0.0 => "sim-summary-value sv-primary",
                                Some(_) => "sim-summary-value sv-danger",
                                None => "sim-summary-value",
                            }
                        }>
                            { avg_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>



                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total TP ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value tp-col">
                            { price_unit.display_amount(total_tp_amount) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total SL ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value sl-col">
                            { price_unit.display_amount(total_sl_amount) }
                        </div>
                    </div>

                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg Hold" }</div>
                        <div class="sim-summary-value">
                            { avg_holding_secs.map(|s| format_age(s as i64)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Best" }</div>
                        <div class="sim-summary-value tp-col">
                            { best_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Worst" }</div>
                        <div class="sim-summary-value sl-col">
                            { worst_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
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
                Some(v) if v >= 0.0 => html! { <span class="tp-col">{ price_unit.display_amount(v) }</span> },
                Some(v)             => html! { <span class="sl-col">{ price_unit.display_amount(v) }</span> },
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
                    <td class="num-col">{ price_unit.display_price(t.entry_price) }</td>
                    <td class="dim-col">{ entry_time_str }</td>
                    <td class="num-col">{ t.exit_price.map(|p| price_unit.display_price(p)).unwrap_or_else(|| "—".into()) }</td>
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
                        <span class="matched-count-badge">{ result.tokens.len() }</span>
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
                                        <th>{ format!("PnL ({})", price_unit.unit_label()) }</th>
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
    let modal_title = if is_edit {
        "Edit TPSL Rule"
    } else {
        "New TPSL Rule"
    };
    let modal_visible = !matches!(&*modal_mode, ModalMode::None);

    {
        let modal_visible = modal_visible;
        use_effect_with(modal_visible, move |visible| {
            if let Some(window) = web_sys::window() {
                if let Some(body) = window.document().and_then(|d| d.body()) {
                    let class_name = body.class_name();
                    let mut classes: Vec<&str> = class_name.split_whitespace().collect();
                    if *visible {
                        if !classes.iter().any(|&c| c == "modal-open") {
                            classes.push("modal-open");
                        }
                    } else {
                        classes.retain(|&c| c != "modal-open");
                    }
                    body.set_class_name(&classes.join(" "));
                }
            }
            || ()
        });
    }

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
                                        <th>{ "Max SOL" }</th>
                                        <th>{ "Spendable" }</th>
                                        <th>{ "Max Holding" }</th>
                                        <th>{ "Total Max" }</th>
                                        <th>{ "IX" }</th>
                                        <th>{ "Labels" }</th>
                                        <th>{ "Buy Amt" }</th>
                                        <th>{ "TP" }</th>
                                        <th>{ "SL" }</th>
                                        <th>{ "Tolerance" }</th>
                                        <th>{ "Status" }</th>
                                        <th>{ "Actions" }</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    if filtered.is_empty() {
                                        <tr><td colspan="16" class="no-data">{ "No rules found" }</td></tr>
                                    } else {
                                        { rule_rows }
                                    }
                                </tbody>
                            </table>
                        </div>
                    </div>
                }

                // ── Trading tokens table for selected active rule ─────────────
                {{
                    let selected_rule = selected_rule_id.as_ref().and_then(|id| rules.iter().find(|r| &r.id == id));
                    if let Some(rule) = selected_rule {
                        if rule.is_active {
                            html! {
                                <div class="table-wrapper" style="margin-top: 24px;">
                                    <div class="table-scroll">
                                        <table class="trade-table">
                                            <thead>
                                                <tr>
                                                    <th class="th-row-num">{ "#" }</th>
                                                    <th>{ "Symbol" }</th>
                                                    <th>{ "Entry Price" }</th>
                                                    <th>{ "Entry Time" }</th>
                                                    <th>{ "Exit Price" }</th>
                                                    <th>{ "Exit Time" }</th>
                                                    <th>{ "Holding" }</th>
                                                    <th>{ "PnL%" }</th>
                                                    <th>{ format!("PnL ({})", price_unit.unit_label()) }</th>
                                                    <th>{ "Reason" }</th>
                                                    <th>{ "Trades" }</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                if *rule_positions_loading {
                                                    <tr><td colspan="11" class="no-data">{ "Loading tokens…" }</td></tr>
                                                } else if let Some(err) = &*rule_positions_error {
                                                    <tr><td colspan="11" class="inline-error">{ err }</td></tr>
                                                } else if rule_positions.is_empty() {
                                                    <tr><td colspan="11" class="no-data">{ "No trading tokens for this rule." }</td></tr>
                                                } else {
                                                    { rule_positions.iter().enumerate().map(|(i, t)| {
                                                        let entry_time_str = t.created_at.get(..16).unwrap_or(&t.created_at).replace('T', " ");
                                                        let exit_time_str = t.updated_at.get(..16).unwrap_or(&t.updated_at).replace('T', " ");
                                                        let pnl_pct_html = match t.pnl_percent {
                                                            Some(v) if v >= 0.0 => html! { <span class="tp-col">{ format!("{:+.1}%", v) }</span> },
                                                            Some(v)             => html! { <span class="sl-col">{ format!("{:.1}%", v)  }</span> },
                                                            None                => html! { <span class="dim-col">{ "—" }</span> },
                                                        };
                                                        let pnl_val_html = match t.exit_price {
                                                            Some(_) if t.pnl_percent.unwrap_or(0.0) >= 0.0 => html! { <span class="tp-col">{ price_unit.display_amount(t.exit_amount.unwrap_or(0.0)) }</span> },
                                                            Some(_) => html! { <span class="sl-col">{ price_unit.display_amount(t.exit_amount.unwrap_or(0.0)) }</span> },
                                                            None => html! { <span class="dim-col">{ "—" }</span> },
                                                        };
                                                        let reason_html = match t.status.as_str() {
                                                            "TakeProfit" => html! { <span class="tp-col">{ "TP" }</span> },
                                                            "StopLoss"   => html! { <span class="sl-col">{ "SL" }</span> },
                                                            _            => html! { <span class="dim-col">{ &t.status }</span> },
                                                        };
                                                        html! {
                                                            <tr key={t.mint.clone()}>
                                                                <td class="row-num">{ i + 1 }</td>
                                                                <td>
                                                                    <a href={format!("https://gmgn.ai/sol/token/{}", t.mint)}
                                                                        target="_blank" rel="noreferrer" class="symbol-link-inline">
                                                                        { &t.mint[..6] }
                                                                    </a>
                                                                </td>
                                                                <td class="num-col">{ price_unit.display_price(t.entry_price) }</td>
                                                                <td class="dim-col">{ entry_time_str }</td>
                                                                <td class="num-col">{ t.exit_price.map(|p| price_unit.display_price(p)).unwrap_or_else(|| "—".into()) }</td>
                                                                <td class="dim-col">{ exit_time_str }</td>
                                                                <td class="dim-col">{ t.exit_amount.map(|amt| format_decimal_trim(amt, 3)).unwrap_or_else(|| "—".into()) }</td>
                                                                <td>{ pnl_pct_html }</td>
                                                                <td>{ pnl_val_html }</td>
                                                                <td>{ reason_html }</td>
                                                                <td class="dim-col">{ t.exit_tx.as_ref().map(|_| 1).unwrap_or(0) }</td>
                                                            </tr>
                                                        }
                                                    }).collect::<Html>() }
                                                }
                                            </tbody>
                                        </table>
                                    </div>
                                </div>
                            }
                        } else { html! {} }
                    } else { html! {} }
                }}

                // ── Simulation tokens panel ───────────────────────────────────
                { sim_panel }

                // ── Add / Edit modal ──────────────────────────────────────────
                <Modal title={modal_title.to_string()} visible={modal_visible} on_close={close_modal}>
                    <div class="rule-form">
                        <div class="rule-form-grid">

                            <label class="form-field form-field-full">
                                <span class="form-label" style="color:var(--primary)">{ "Rule Name" }</span>
                                <input type="text" class="form-input" value={(*f_name).clone()}
                                    oninput={oninput!(f_name)} placeholder="e.g. Sniper 0.5 SOL" />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Initial Buy SOL" }</span>
                                <input type="number" step="0.001"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_initial_buy).clone()} oninput={oninput!(f_initial_buy)}
                                    placeholder="0.5" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Tolerance %" }</span>
                                <input type="number" step="0.1"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_tolerance).clone()} oninput={oninput!(f_tolerance)} placeholder="0" readonly={is_edit && !*f_allow_edit_params} />
                                <div class="form-hint">{ "Tolerance % — 0 = exact match; e.g. 10 allows ±10% for numeric rule criteria (Init Buy, CU Limit, CU Price, Max SOL Cost, Spendable SOL In)." }</div>
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "CU Limit" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_limit).clone()} oninput={oninput!(f_cu_limit)}
                                    placeholder="e.g. 200000" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "CU Price" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_price).clone()} oninput={oninput!(f_cu_price)}
                                    placeholder="e.g. 1000000" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "Max SOL Cost" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number" step="0.001"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_max_sol_cost).clone()} oninput={oninput!(f_max_sol_cost)}
                                    placeholder="0.5" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "Spendable SOL In" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number" step="0.001"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_spendable_sol_in).clone()} oninput={oninput!(f_spendable_sol_in)}
                                    placeholder="1.0" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "Max Holding Tokens" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number" step="1"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_max_holding_tokens).clone()} oninput={oninput!(f_max_holding_tokens)}
                                    placeholder="5" readonly={is_edit && !*f_allow_edit_params} />
                                <div class="form-hint">{ "Optional limit on how many matched tokens may be held simultaneously by this rule. This limit applies to live strategy execution and rule simulation." }</div>
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">
                                    { "Total Max Trade Tokens" }
                                    <span class="form-opt">{ " opt" }</span>
                                </span>
                                <input type="number" step="1"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_total_max_trade_tokens).clone()} oninput={oninput!(f_total_max_trade_tokens)}
                                    placeholder="10" readonly={is_edit && !*f_allow_edit_params} />
                                <div class="form-hint">{ "Optional limit on the total number of tokens this rule may trade over time. Once reached, the rule will stop creating new positions." }</div>
                            </label>

                            <div class="form-field form-field-full">
                                <div class="form-field-label-row">
                                    <span class="form-label" style="color:var(--text-dim)">
                                            { "Instruction Labels" }
                                            <span class="form-opt">{ " comma-separated or JSON array, opt" }</span>
                                        </span>
                                    { if is_edit {
                                        html! {
                                            <button type="button" class="form-action-btn" onclick={toggle_edit_params.clone()}
                                                title={if *f_allow_edit_params { "Lock rule criteria" } else { "Unlock rule criteria" }}>
                                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                                                    { if *f_allow_edit_params {
                                                        html! {
                                                            <>
                                                                <rect x="3" y="11" width="18" height="11" rx="2" />
                                                                <path d="M7 11V7a5 5 0 0110 0v4" />
                                                            </>
                                                        }
                                                    } else {
                                                        html! {
                                                            <>
                                                                <rect x="3" y="11" width="18" height="11" rx="2" />
                                                                <path d="M7 11V7a5 5 0 0110 0v4" />
                                                                <line x1="9" y1="15" x2="9" y2="18" />
                                                                <line x1="15" y1="15" x2="15" y2="18" />
                                                            </>
                                                        }
                                                    } }
                                                </svg>
                                            </button>
                                        }
                                    } else { html! {} }}
                                    { if !is_edit {
                                        html! {
                                            <button type="button" class="form-action-btn" onclick={populate_example_labels}
                                                title="Insert example instruction labels">
                                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                                                    <rect x="9" y="9" width="13" height="13" rx="2" />
                                                    <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
                                                </svg>
                                            </button>
                                        }
                                    } else { html! {} }}
                                </div>
                                <div class="form-input-row">
                                    <textarea rows="4"
                                        class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                        value={(*f_ix_labels).clone()}
                                        oninput={oninput!(f_ix_labels)}
                                        placeholder="[\"Compute Budget: SetComputeUnitLimit\", \"Pump.Fun: Buy\"]" readonly={is_edit && !*f_allow_edit_params}>
                                    </textarea>
                                </div>
                                <span class="form-hint">
                                    { "Paste comma-separated labels or a JSON-style array. Click the icon above to populate a sample list." }
                                </span>
                            </div>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--primary)">{ "Buy Amount (SOL)" }</span>
                                <input type="number" step="0.001" class="form-input"
                                    value={(*f_buy_amount).clone()} oninput={oninput!(f_buy_amount)} placeholder="0.1" />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--primary)">{ "Take Profit %" }</span>
                                <input type="number" step="1" class="form-input form-input-tp"
                                    value={(*f_take_profit).clone()} oninput={oninput!(f_take_profit)} placeholder="50" />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--primary)">{ "Stop Loss %" }</span>
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

                        { if is_edit {
                            html! {
                                <div class="form-hint form-hint-warning">
                                    { if *f_allow_edit_params {
                                        "Rule criteria are unlocked for edit. Changes to entry criteria will be saved when you click Save Rule."
                                    } else {
                                        "Rule criteria are locked. Click Unlock rule criteria to edit Initial Buy, CU Limit, CU Price, Max SOL Cost, Spendable SOL In, Max Holding Tokens, and Labels."
                                    } }
                                </div>
                            }
                        } else {
                            html! {}
                        }}
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
