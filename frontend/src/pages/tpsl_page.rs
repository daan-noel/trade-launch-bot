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
use std::rc::Rc;
use gloo::timers::callback::Interval;
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::modal::Modal;
use crate::components::{Column, DataTable, Header, SortKey};
use crate::services::api::{
    create_tpsl_rule, delete_tpsl_rule, fetch_matched_tokens, fetch_rule_positions,
    fetch_tpsl_rules, simulate_tpsl_rule, update_tpsl_rule, CreateRuleRequest,
    MatchedTokenRecord, RulePositionRecord, RuleRecord, SimulatedTokenResult, UpdateRuleRequest,
    POLL_INTERVAL_MS,
};
use crate::state::{PriceUnitContext, TpslAction, TpslContext};
use crate::utils::format::{format_age, format_decimal_trim};

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

#[function_component(TpslPage)]
pub fn tpsl_page() -> Html {
    let tpsl =
        use_context::<TpslContext>().expect("TpslProvider must be mounted above TpslPage");

    // ── UI-only state ─────────────────────────────────────────────────────────
    let selected_rule_id = use_state(|| Option::<String>::None);

    // ── Modal / form ──────────────────────────────────────────────────────────
    let modal_mode = use_state(|| ModalMode::None);
    let f_name = use_state(String::new);
    let f_initial_buy = use_state(String::new);
    let f_cu_limit = use_state(String::new);
    let f_cu_price = use_state(String::new);
    let f_ix_labels = use_state(String::new);
    let f_buy_amount = use_state(String::new);
    let f_trade_mode = use_state(|| "paper".to_string());
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

    // ── Matched tokens ────────────────────────────────────────────────────────
    let matched_result = use_state(|| Option::<(String, Vec<MatchedTokenRecord>)>::None);
    let matched_error = use_state(|| Option::<String>::None);
    let matched_loading = use_state(|| false);

    // ── Poll tick ─────────────────────────────────────────────────────────────
    let tick = use_state(|| 0u32);
    let tick_ref = use_mut_ref(|| 0u32);
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

    // ── Fetch rules on mount; silently refresh on every tick ──────────────────
    // TpslProvider uses use_reducer_eq: dispatch is a no-op when data is unchanged.
    {
        let tpsl = tpsl.clone();
        use_effect_with(*tick, move |tick_val| {
            let is_initial = *tick_val == 0;
            if is_initial {
                tpsl.dispatch(TpslAction::SetLoading);
            }
            spawn_local(async move {
                match fetch_tpsl_rules().await {
                    Ok(fetched) => tpsl.dispatch(TpslAction::SetRules(fetched)),
                    Err(err) if is_initial => tpsl.dispatch(TpslAction::SetError(err)),
                    Err(_) => {}
                }
            });
            || ()
        });
    }

    // ── Fetch positions for selected rule (any status); silently refresh on tick ──
    let prev_selected_for_pos = use_mut_ref(|| Option::<String>::None);
    {
        let tpsl = tpsl.clone();
        let prev_selected_for_pos = prev_selected_for_pos.clone();
        use_effect_with(
            ((*selected_rule_id).clone(), *tick),
            move |(selected, _tick)| {
                let selection_changed = *prev_selected_for_pos.borrow() != *selected;
                *prev_selected_for_pos.borrow_mut() = selected.clone();
                if let Some(rule_id) = selected.as_ref() {
                    if selection_changed {
                        tpsl.dispatch(TpslAction::SetPositionsLoading);
                    }
                    let rule_id = rule_id.clone();
                    let tpsl = tpsl.clone();
                    spawn_local(async move {
                        match fetch_rule_positions(&rule_id).await {
                            Ok(positions) => tpsl.dispatch(TpslAction::SetPositions(positions)),
                            Err(err) if selection_changed => {
                                tpsl.dispatch(TpslAction::SetPositionsError(err))
                            }
                            Err(_) => {}
                        }
                    });
                } else {
                    tpsl.dispatch(TpslAction::ClearPositions);
                }
                || ()
            },
        );
    }

    // ── Helpers: open modals ──────────────────────────────────────────────────
    let f_trade_mode_for_add = f_trade_mode.clone();
    let open_add = {
        let (modal_mode, f_name, f_initial_buy, f_cu_limit, f_cu_price, f_trade_mode) = (
            modal_mode.clone(),
            f_name.clone(),
            f_initial_buy.clone(),
            f_cu_limit.clone(),
            f_cu_price.clone(),
            f_trade_mode_for_add,
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
            f_trade_mode.set(String::from("paper"));
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
            f_trade_mode,
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
            f_trade_mode.clone(),
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
            f_trade_mode.set(rule.trade_mode.clone());
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
        let modal_mode = modal_mode.clone();
        let tpsl = tpsl.clone();
        let form_error = form_error.clone();
        let form_loading = form_loading.clone();
        let f_name = f_name.clone();
        let f_initial_buy = f_initial_buy.clone();
        let f_cu_limit = f_cu_limit.clone();
        let f_cu_price = f_cu_price.clone();
        let f_ix_labels = f_ix_labels.clone();
        let f_max_sol_cost = f_max_sol_cost.clone();
        let f_spendable_sol_in = f_spendable_sol_in.clone();
        let f_max_holding_tokens = f_max_holding_tokens.clone();
        let f_total_max_trade_tokens = f_total_max_trade_tokens.clone();
        let f_tolerance = f_tolerance.clone();
        let f_buy_amount = f_buy_amount.clone();
        let f_take_profit = f_take_profit.clone();
        let f_stop_loss = f_stop_loss.clone();
        let f_trade_mode = f_trade_mode.clone();
        Callback::from(move |_: MouseEvent| {
            let modal_mode_val = (*modal_mode).clone();
            let tpsl = tpsl.clone();
            let form_error = form_error.clone();
            let form_loading = form_loading.clone();
            let modal_mode = modal_mode.clone();
            let name = (*f_name).clone();
            let initial_buy_s = (*f_initial_buy).clone();
            let cu_limit_s = (*f_cu_limit).clone();
            let cu_price_s = (*f_cu_price).clone();
            let ix_labels_s = (*f_ix_labels).clone();
            let max_sol_cost_s = (*f_max_sol_cost).clone();
            let spendable_sol_in_s = (*f_spendable_sol_in).clone();
            let max_holding_tokens_s = (*f_max_holding_tokens).clone();
            let total_max_trade_tokens_s = (*f_total_max_trade_tokens).clone();
            let tolerance_s = (*f_tolerance).clone();
            let buy_amount_s = (*f_buy_amount).clone();
            let take_profit_s = (*f_take_profit).clone();
            let stop_loss_s = (*f_stop_loss).clone();
            let f_trade_mode = (*f_trade_mode).clone();
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

                match &modal_mode_val {
                    ModalMode::Add => {
                        let p_initial_buy_sol = if initial_buy_s.trim().is_empty() {
                            None
                        } else {
                            match initial_buy_s.trim().parse::<f64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid initial buy SOL".into()));
                                    form_loading.set(false);
                                    return ();
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
                                Ok(Value::Array(arr)) => arr,
                                _ => vec![],
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
                        let p_total_max_trade_tokens = if total_max_trade_tokens_s.trim().is_empty()
                        {
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
                                    form_error.set(Some("Invalid tolerance".into()));
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
                            trade_mode: f_trade_mode.to_string(),
                            buy_amount,
                            take_profit,
                            stop_loss,
                            tolerance_pct: p_tolerance,
                        };
                        match create_tpsl_rule(&req).await {
                            Ok(new_rule) => {
                                tpsl.dispatch(TpslAction::AddRule(new_rule));
                                modal_mode.set(ModalMode::None);
                            }
                            Err(err) => form_error.set(Some(err)),
                        }
                    }
                    ModalMode::Edit(rule) => {
                        let rule = rule.clone();
                        let p_initial_buy_sol = Some(Some(if initial_buy_s.trim().is_empty() {
                            0.0
                        } else {
                            initial_buy_s.trim().parse::<f64>().unwrap_or(0.0)
                        }));
                        let p_cu_limit = Some(Some(if cu_limit_s.trim().is_empty() {
                            0
                        } else {
                            cu_limit_s.trim().parse::<u64>().unwrap_or(0)
                        }));
                        let p_cu_price = Some(Some(if cu_price_s.trim().is_empty() {
                            0
                        } else {
                            cu_price_s.trim().parse::<u64>().unwrap_or(0)
                        }));
                        let p_ix_labels = if ix_labels_s.trim().is_empty() {
                            Some(Some(Value::Array(vec![])))
                        } else {
                            let labels_vec: Vec<Value> = if ix_labels_s.trim().starts_with('[') {
                                match serde_json::from_str::<Value>(ix_labels_s.trim()) {
                                    Ok(Value::Array(arr)) => arr,
                                    _ => vec![],
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
                        let p_max_sol_cost = Some(Some(if max_sol_cost_s.trim().is_empty() {
                            0.0
                        } else {
                            max_sol_cost_s.trim().parse::<f64>().unwrap_or(0.0)
                        }));
                        let p_spendable_sol_in =
                            Some(Some(if spendable_sol_in_s.trim().is_empty() {
                                0.0
                            } else {
                                spendable_sol_in_s.trim().parse::<f64>().unwrap_or(0.0)
                            }));
                        let p_max_holding_tokens =
                            Some(Some(if max_holding_tokens_s.trim().is_empty() {
                                0
                            } else {
                                max_holding_tokens_s.trim().parse::<u64>().unwrap_or(0)
                            }));
                        let p_total_max_trade_tokens =
                            Some(Some(if total_max_trade_tokens_s.trim().is_empty() {
                                0
                            } else {
                                total_max_trade_tokens_s.trim().parse::<u64>().unwrap_or(0)
                            }));
                        let p_tolerance = if tolerance_s.trim().is_empty() {
                            None
                        } else {
                            match tolerance_s.trim().parse::<f64>() {
                                Ok(v) => Some(v),
                                Err(_) => {
                                    form_error.set(Some("Invalid tolerance".into()));
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
                            trade_mode: Some(f_trade_mode.clone()),
                        };
                        match update_tpsl_rule(&rule.id, &req).await {
                            Ok(updated) => {
                                tpsl.dispatch(TpslAction::UpdateRule(updated));
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
        let tpsl = tpsl.clone();
        Callback::from(move |rule: RuleRecord| {
            let tpsl = tpsl.clone();
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
                    trade_mode: None,
                };
                if let Ok(updated) = update_tpsl_rule(&rule.id, &req).await {
                    tpsl.dispatch(TpslAction::UpdateRule(updated));
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
        let (confirm_delete_id, tpsl, delete_loading) = (
            confirm_delete_id.clone(),
            tpsl.clone(),
            delete_loading.clone(),
        );
        Callback::from(move |_: MouseEvent| {
            let rule_id = match (*confirm_delete_id).clone() {
                Some(id) => id,
                None => return,
            };
            let (confirm_delete_id, tpsl, delete_loading) = (
                confirm_delete_id.clone(),
                tpsl.clone(),
                delete_loading.clone(),
            );
            delete_loading.set(true);
            spawn_local(async move {
                if delete_tpsl_rule(&rule_id).await.is_ok() {
                    tpsl.dispatch(TpslAction::RemoveRule(rule_id));
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

    // ── Matched tokens ────────────────────────────────────────────────────────
    let on_show_matched = {
        let (matched_result, matched_error, matched_loading) = (
            matched_result.clone(),
            matched_error.clone(),
            matched_loading.clone(),
        );
        Callback::from(move |rule: RuleRecord| {
            let (matched_result, matched_error, matched_loading) = (
                matched_result.clone(),
                matched_error.clone(),
                matched_loading.clone(),
            );
            // Toggle off if already showing this rule's results.
            if matched_result.as_ref().map(|(id, _)| id == &rule.id).unwrap_or(false) {
                matched_result.set(None);
                matched_error.set(None);
                return;
            }
            matched_result.set(None);
            matched_error.set(None);
            matched_loading.set(true);
            let rule_id = rule.id.clone();
            spawn_local(async move {
                match fetch_matched_tokens(&rule_id).await {
                    Ok(tokens) => matched_result.set(Some((rule_id, tokens))),
                    Err(err) => matched_error.set(Some(err)),
                }
                matched_loading.set(false);
            });
        })
    };

    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above StrategyPage");

    // ── Column definitions ────────────────────────────────────────────────────

    let rules_cols: Vec<Column<RuleRecord>> = {
        let on_toggle_active = on_toggle_active.clone();
        vec![
            Column { key: "name", label: "Name", render: Rc::new(|r: &RuleRecord| html!{<span class="rule-name-cell">{r.rule_name.clone()}</span>}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Str(r.rule_name.clone()))), search_value: Rc::new(|r: &RuleRecord| r.rule_name.clone()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "init_buy", label: "Init Buy", render: Rc::new(|r: &RuleRecord| html!{dash_f(r.p_initial_buy_sol.unwrap_or(0.0), 15)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.p_initial_buy_sol.unwrap_or(0.0)))), search_value: Rc::new(|r: &RuleRecord| r.p_initial_buy_sol.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "cu_limit", label: "CU Lim", render: Rc::new(|r: &RuleRecord| html!{dash(r.p_cu_limit)}), sort_value: Some(Rc::new(|r: &RuleRecord| r.p_cu_limit.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &RuleRecord| r.p_cu_limit.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "cu_price", label: "CU Price", render: Rc::new(|r: &RuleRecord| html!{dash(r.p_cu_price)}), sort_value: Some(Rc::new(|r: &RuleRecord| r.p_cu_price.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &RuleRecord| r.p_cu_price.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "max_sol", label: "Max SOL", render: Rc::new(|r: &RuleRecord| html!{dash_f(r.p_max_sol_cost.unwrap_or(0.0), 3)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.p_max_sol_cost.unwrap_or(0.0)))), search_value: Rc::new(|r: &RuleRecord| r.p_max_sol_cost.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "spendable", label: "Spendable", render: Rc::new(|r: &RuleRecord| html!{dash_f(r.p_spendable_sol_in.unwrap_or(0.0), 3)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.p_spendable_sol_in.unwrap_or(0.0)))), search_value: Rc::new(|r: &RuleRecord| r.p_spendable_sol_in.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "max_hold", label: "Max Hold", render: Rc::new(|r: &RuleRecord| html!{dash(r.p_max_holding_tokens)}), sort_value: Some(Rc::new(|r: &RuleRecord| r.p_max_holding_tokens.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &RuleRecord| r.p_max_holding_tokens.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "total_max", label: "Total Max", render: Rc::new(|r: &RuleRecord| html!{dash(r.p_total_max_trade_tokens)}), sort_value: Some(Rc::new(|r: &RuleRecord| r.p_total_max_trade_tokens.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &RuleRecord| r.p_total_max_trade_tokens.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "labels", label: "Labels", render: Rc::new(|r: &RuleRecord| { let n = r.p_ix_labels.as_array().map(|a| a.len()).unwrap_or(0); let tooltip = r.p_ix_labels.as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default(); let display = if n > 0 { n.to_string() } else { "-".to_string() }; html!{<span title={tooltip} class="num-col">{display}</span>} }), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.p_ix_labels.as_array().map(|a| a.len()).unwrap_or(0) as f64))), search_value: Rc::new(|r: &RuleRecord| r.p_ix_labels.as_array().map(|a| a.iter().filter_map(|v|v.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "buy_amt", label: "Buy Amt", render: Rc::new(|r: &RuleRecord| html!{dash_f(r.buy_amount, 15)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.buy_amount))), search_value: Rc::new(|r: &RuleRecord| r.buy_amount.to_string()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "tp", label: "TP", render: Rc::new(|r: &RuleRecord| html!{dash_percent(r.take_profit)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.take_profit))), search_value: Rc::new(|r: &RuleRecord| r.take_profit.to_string()), cell_class: Some("tp-col"), sortable: true, default_visible: true, width: None },
            Column { key: "sl", label: "SL", render: Rc::new(|r: &RuleRecord| html!{dash_percent(r.stop_loss)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.stop_loss))), search_value: Rc::new(|r: &RuleRecord| r.stop_loss.to_string()), cell_class: Some("sl-col"), sortable: true, default_visible: true, width: None },
            Column { key: "tol", label: "Tolerance", render: Rc::new(|r: &RuleRecord| html!{dash_percent(r.tolerance_pct)}), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Num(r.tolerance_pct))), search_value: Rc::new(|r: &RuleRecord| r.tolerance_pct.to_string()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "mode", label: "Mode", render: Rc::new(|r: &RuleRecord| { let cls = if r.trade_mode == "real" {"mode-pill mode-real"} else {"mode-pill mode-paper"}; let lbl = if r.trade_mode == "real" {"Real"} else {"Paper"}; html!{<span class={cls}>{lbl}</span>} }), sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Str(r.trade_mode.clone()))), search_value: Rc::new(|r: &RuleRecord| r.trade_mode.clone()), cell_class: Some("mode-col"), sortable: true, default_visible: true, width: None },
            Column { key: "status", label: "Status", render: {
                let on_toggle_active = on_toggle_active.clone();
                Rc::new(move |r: &RuleRecord| {
                    let cls = if r.is_active {"status-pill status-active"} else {"status-pill status-inactive"};
                    let lbl = if r.is_active {"● Active"} else {"○ Inactive"};
                    let rule = r.clone();
                    let cb = on_toggle_active.clone();
                    let onclick = Callback::from(move |e: MouseEvent| { e.stop_propagation(); cb.emit(rule.clone()); });
                    html!{<button class={cls} onclick={onclick} title="Toggle active/inactive">{lbl}</button>}
                })
            }, sort_value: Some(Rc::new(|r: &RuleRecord| SortKey::Str(r.is_active.to_string()))), search_value: Rc::new(|r: &RuleRecord| r.is_active.to_string()), cell_class: Some("status-col"), sortable: true, default_visible: true, width: None },
        ]
    };

    let rules_actions: Rc<dyn Fn(&RuleRecord) -> Html> = {
        let open_edit = open_edit.clone();
        let on_request_delete = on_request_delete.clone();
        let on_simulate = on_simulate.clone();
        let on_show_matched = on_show_matched.clone();
        let confirm_delete_id = confirm_delete_id.clone();
        let on_confirm_delete = on_confirm_delete.clone();
        let on_cancel_delete = on_cancel_delete.clone();
        let delete_loading = delete_loading.clone();
        let simulate_loading = simulate_loading.clone();
        let matched_loading = matched_loading.clone();
        let matched_result = matched_result.clone();
        Rc::new(move |rule: &RuleRecord| {
            let is_confirming = confirm_delete_id.as_ref().map(|id| id == &rule.id).unwrap_or(false);
            let matched_active = matched_result.as_ref().map(|(id, _)| id == &rule.id).unwrap_or(false);
            let on_edit_cb = { let oe = open_edit.clone(); let r = rule.clone(); Callback::from(move |e: MouseEvent| { e.stop_propagation(); oe.emit(r.clone()); }) };
            let on_del_cb = { let od = on_request_delete.clone(); let id = rule.id.clone(); Callback::from(move |e: MouseEvent| { e.stop_propagation(); od.emit(id.clone()); }) };
            let on_sim_cb = { let os = on_simulate.clone(); let r = rule.clone(); Callback::from(move |e: MouseEvent| { e.stop_propagation(); os.emit(r.clone()); }) };
            let on_match_cb = { let om = on_show_matched.clone(); let r = rule.clone(); Callback::from(move |e: MouseEvent| { e.stop_propagation(); om.emit(r.clone()); }) };
            if is_confirming {
                html! {
                    <>
                        <span class="confirm-text">{"Delete?"}</span>
                        <button class="act-btn act-danger" onclick={on_confirm_delete.clone()} disabled={*delete_loading}>{"Yes"}</button>
                        <button class="act-btn" onclick={on_cancel_delete.clone()}>{"No"}</button>
                    </>
                }
            } else {
                html! {
                    <>
                        <button class="act-btn act-edit" onclick={on_edit_cb} disabled={rule.is_active} title={if rule.is_active {"Cannot edit active rules"} else {"Edit rule"}}>{"Edit"}</button>
                        <button class="act-btn act-danger" onclick={on_del_cb} disabled={rule.is_active} title={if rule.is_active {"Cannot delete active rules"} else {"Delete rule"}}>{"Del"}</button>
                        <button class="act-btn act-sim" onclick={on_sim_cb} disabled={*simulate_loading} title="Run simulation">{"▶"}</button>
                        <button class={if matched_active {"act-btn act-list act-list-active"} else {"act-btn act-list"}} onclick={on_match_cb} disabled={*matched_loading} title="Show matched tokens">{"⊞"}</button>
                    </>
                }
            }
        })
    };

    let positions_cols: Vec<Column<RulePositionRecord>> = {
        let pu = price_unit.clone();
        vec![
            Column { key: "mint", label: "Mint", render: Rc::new(|r: &RulePositionRecord| html!{<a href={format!("https://gmgn.ai/sol/token/{}",&r.mint)} target="_blank" rel="noreferrer" class="symbol-link-inline">{r.mint.get(..6).unwrap_or(&r.mint)}</a>}), sort_value: None, search_value: Rc::new(|r: &RulePositionRecord| r.mint.clone()), cell_class: None, sortable: false, default_visible: true, width: None },
            Column { key: "entry_price", label: "Entry Price", render: { let p = pu.clone(); Rc::new(move |r: &RulePositionRecord| html!{p.display_price(r.entry_price)}) }, sort_value: Some(Rc::new(|r: &RulePositionRecord| SortKey::Num(r.entry_price))), search_value: Rc::new(|r: &RulePositionRecord| r.entry_price.to_string()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "entry_time", label: "Entry Time", render: Rc::new(|r: &RulePositionRecord| { let s = r.entry_time.as_deref().map(|s| s.get(..16).unwrap_or(s).replace('T'," ")).unwrap_or_else(||"—".into()); html!{s} }), sort_value: Some(Rc::new(|r: &RulePositionRecord| SortKey::Str(r.entry_time.clone().unwrap_or_default()))), search_value: Rc::new(|r: &RulePositionRecord| r.entry_time.clone().unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "exit_price", label: "Exit Price", render: { let p = pu.clone(); Rc::new(move |r: &RulePositionRecord| html!{r.exit_price.map(|v| p.display_price(v)).unwrap_or_else(||"—".into())}) }, sort_value: Some(Rc::new(|r: &RulePositionRecord| r.exit_price.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|r: &RulePositionRecord| r.exit_price.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "exit_time", label: "Exit Time", render: Rc::new(|r: &RulePositionRecord| { let s = r.exit_time.as_deref().map(|s| s.get(..16).unwrap_or(s).replace('T'," ")).unwrap_or_else(||"—".into()); html!{s} }), sort_value: Some(Rc::new(|r: &RulePositionRecord| SortKey::Str(r.exit_time.clone().unwrap_or_default()))), search_value: Rc::new(|r: &RulePositionRecord| r.exit_time.clone().unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "holding", label: "Holding", render: Rc::new(|r: &RulePositionRecord| html!{r.exit_amount.map(|v| format_decimal_trim(v,3)).unwrap_or_else(||"—".into())}), sort_value: None, search_value: Rc::new(|_| String::new()), cell_class: Some("dim-col"), sortable: false, default_visible: true, width: None },
            Column { key: "pnl_pct", label: "PnL%", render: Rc::new(|r: &RulePositionRecord| match r.pnl_percent { Some(v) if v >= 0.0 => html!{<span class="tp-col">{format!("{:+.1}%",v)}</span>}, Some(v) => html!{<span class="sl-col">{format!("{:.1}%",v)}</span>}, None => html!{<span class="dim-col">{"—"}</span>} }), sort_value: Some(Rc::new(|r: &RulePositionRecord| r.pnl_percent.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|r: &RulePositionRecord| r.pnl_percent.map(|v|v.to_string()).unwrap_or_default()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "pnl_sol", label: "PnL", render: { let p = pu.clone(); Rc::new(move |r: &RulePositionRecord| match r.exit_price { Some(_) if r.pnl_percent.unwrap_or(0.0) >= 0.0 => html!{<span class="tp-col">{p.display_amount(r.exit_amount.unwrap_or(0.0))}</span>}, Some(_) => html!{<span class="sl-col">{p.display_amount(r.exit_amount.unwrap_or(0.0))}</span>}, None => html!{<span class="dim-col">{"—"}</span>} }) }, sort_value: None, search_value: Rc::new(|_| String::new()), cell_class: None, sortable: false, default_visible: true, width: None },
            Column { key: "status", label: "Status", render: Rc::new(|r: &RulePositionRecord| { let h = match r.status.as_str() { "TakeProfit" => html!{<span class="tp-col">{"TP"}</span>}, "StopLoss" => html!{<span class="sl-col">{"SL"}</span>}, s => html!{<span class="dim-col">{s.to_string()}</span>} }; h }), sort_value: Some(Rc::new(|r: &RulePositionRecord| SortKey::Str(r.status.clone()))), search_value: Rc::new(|r: &RulePositionRecord| r.status.clone()), cell_class: None, sortable: true, default_visible: true, width: None },
        ]
    };

    let matched_cols: Vec<Column<MatchedTokenRecord>> = vec![
        Column { key: "symbol", label: "Symbol", render: Rc::new(|r: &MatchedTokenRecord| html!{<a href={format!("https://gmgn.ai/sol/token/{}",&r.mint)} target="_blank" rel="noreferrer" class="symbol-link-inline">{r.symbol.clone()}</a>}), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| SortKey::Str(r.symbol.clone()))), search_value: Rc::new(|r: &MatchedTokenRecord| format!("{} {}", r.symbol, r.name)), cell_class: None, sortable: true, default_visible: true, width: None },
        Column { key: "name", label: "Name", render: Rc::new(|r: &MatchedTokenRecord| html!{r.name.clone()}), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| SortKey::Str(r.name.clone()))), search_value: Rc::new(|r: &MatchedTokenRecord| r.name.clone()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
        Column { key: "created", label: "Created", render: Rc::new(|r: &MatchedTokenRecord| { let s = r.created_at.get(..16).unwrap_or(&r.created_at).replace('T'," "); html!{s} }), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| SortKey::Str(r.created_at.clone()))), search_value: Rc::new(|r: &MatchedTokenRecord| r.created_at.clone()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
        Column { key: "init_buy", label: "Init Buy (SOL)", render: Rc::new(|r: &MatchedTokenRecord| html!{r.initial_buy_sol.map(|v| format!("{:.4}",v)).unwrap_or_else(||"—".into())}), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| r.initial_buy_sol.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|r: &MatchedTokenRecord| r.initial_buy_sol.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
        Column { key: "cu_limit", label: "CU Limit", render: Rc::new(|r: &MatchedTokenRecord| html!{r.cu_limit.map(|v|v.to_string()).unwrap_or_else(||"—".into())}), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| r.cu_limit.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &MatchedTokenRecord| r.cu_limit.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
        Column { key: "cu_price", label: "CU Price", render: Rc::new(|r: &MatchedTokenRecord| html!{r.cu_price.map(|v|v.to_string()).unwrap_or_else(||"—".into())}), sort_value: Some(Rc::new(|r: &MatchedTokenRecord| r.cu_price.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|r: &MatchedTokenRecord| r.cu_price.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
    ];

    let sim_cols: Vec<Column<SimulatedTokenResult>> = {
        let pu = price_unit.clone();
        vec![
            Column { key: "symbol", label: "Symbol", render: Rc::new(|r: &SimulatedTokenResult| html!{<a href={format!("https://gmgn.ai/sol/token/{}",&r.mint)} target="_blank" rel="noreferrer" class="symbol-link-inline">{r.symbol.clone()}</a>}), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Str(r.symbol.clone()))), search_value: Rc::new(|r: &SimulatedTokenResult| r.symbol.clone()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "entry_price", label: "Entry Price", render: { let p = pu.clone(); Rc::new(move |r: &SimulatedTokenResult| html!{p.display_price(r.entry_price)}) }, sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Num(r.entry_price))), search_value: Rc::new(|r: &SimulatedTokenResult| r.entry_price.to_string()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "entry_time", label: "Entry Time", render: Rc::new(|r: &SimulatedTokenResult| { let s = r.entry_time.get(..16).unwrap_or(&r.entry_time).replace('T'," "); html!{s} }), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Str(r.entry_time.clone()))), search_value: Rc::new(|r: &SimulatedTokenResult| r.entry_time.clone()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "exit_price", label: "Exit Price", render: { let p = pu.clone(); Rc::new(move |r: &SimulatedTokenResult| html!{r.exit_price.map(|v| p.display_price(v)).unwrap_or_else(||"—".into())}) }, sort_value: Some(Rc::new(|r: &SimulatedTokenResult| r.exit_price.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|r: &SimulatedTokenResult| r.exit_price.map(|v|v.to_string()).unwrap_or_default()), cell_class: Some("num-col"), sortable: true, default_visible: true, width: None },
            Column { key: "exit_time", label: "Exit Time", render: Rc::new(|r: &SimulatedTokenResult| { let s = r.exit_time.as_deref().map(|s| s.get(..16).unwrap_or(s).replace('T'," ")).unwrap_or_else(||"—".into()); html!{s} }), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Str(r.exit_time.clone().unwrap_or_default()))), search_value: Rc::new(|r: &SimulatedTokenResult| r.exit_time.clone().unwrap_or_default()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "holding", label: "Holding", render: Rc::new(|r: &SimulatedTokenResult| html!{r.holding_secs.map(format_age).unwrap_or_else(||"—".into())}), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| r.holding_secs.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))), search_value: Rc::new(|_| String::new()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
            Column { key: "pnl_pct", label: "PnL%", render: Rc::new(|r: &SimulatedTokenResult| match r.pnl_percent { Some(v) if v >= 0.0 => html!{<span class="tp-col">{format!("{:+.1}%",v)}</span>}, Some(v) => html!{<span class="sl-col">{format!("{:.1}%",v)}</span>}, None => html!{<span class="dim-col">{"—"}</span>} }), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| r.pnl_percent.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|r: &SimulatedTokenResult| r.pnl_percent.map(|v|v.to_string()).unwrap_or_default()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "pnl_sol", label: "PnL", render: { let p = pu.clone(); Rc::new(move |r: &SimulatedTokenResult| match r.pnl_sol { Some(v) if v >= 0.0 => html!{<span class="tp-col">{p.display_amount(v)}</span>}, Some(v) => html!{<span class="sl-col">{p.display_amount(v)}</span>}, None => html!{<span class="dim-col">{"—"}</span>} }) }, sort_value: Some(Rc::new(|r: &SimulatedTokenResult| r.pnl_sol.map_or(SortKey::Nothing, SortKey::Num))), search_value: Rc::new(|_| String::new()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "reason", label: "Reason", render: Rc::new(|r: &SimulatedTokenResult| match r.exit_reason.as_str() { "TakeProfit" => html!{<span class="tp-col">{"TP"}</span>}, "StopLoss" => html!{<span class="sl-col">{"SL"}</span>}, _ => html!{<span class="dim-col">{"Open"}</span>} }), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Str(r.exit_reason.clone()))), search_value: Rc::new(|r: &SimulatedTokenResult| r.exit_reason.clone()), cell_class: None, sortable: true, default_visible: true, width: None },
            Column { key: "trades", label: "Trades", render: Rc::new(|r: &SimulatedTokenResult| html!{r.total_trades.to_string()}), sort_value: Some(Rc::new(|r: &SimulatedTokenResult| SortKey::Num(r.total_trades as f64))), search_value: Rc::new(|r: &SimulatedTokenResult| r.total_trades.to_string()), cell_class: Some("dim-col"), sortable: true, default_visible: true, width: None },
        ]
    };

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
                    <span class="sim-summary-title">{ "Simulation — " }{ &result.rule_name }</span>
                    <button class="sim-close-btn" onclick={clear_sim_top} title="Close">{ "✕" }</button>
                </div>
                <div class="sim-summary-grid">
                    <div class="sim-summary-stat"><div class="sim-summary-label">{ "Tokens" }</div><div class="sim-summary-value">{ tokens_matched }</div></div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Win Rate" }</div>
                        <div class={if win_rate_pct >= 50.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>{ format!("{:.1}%", win_rate_pct) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "W / L / Open" }</div>
                        <div class="sim-summary-value">
                            <span class="tp-col">{ win_count }</span>{ " / " }<span class="sl-col">{ loss_count }</span>{ " / " }<span class="dim-col">{ open_count }</span>
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total Entry ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value">{ price_unit.display_amount(total_entry_amount) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total Holding ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value">{ price_unit.display_amount(total_holding_amount) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg Entry" }</div>
                        <div class="sim-summary-value">{ avg_entry_amount.map(|v| price_unit.display_amount(v)).unwrap_or_else(|| "—".into()) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total PnL ({})", price_unit.unit_label()) }</div>
                        <div class={if total_pnl_sol >= 0.0 { "sim-summary-value sv-primary" } else { "sim-summary-value sv-danger" }}>{ price_unit.display_amount(total_pnl_sol) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg PnL" }</div>
                        <div class={match avg_pnl_pct { Some(v) if v >= 0.0 => "sim-summary-value sv-primary", Some(_) => "sim-summary-value sv-danger", None => "sim-summary-value" }}>
                            { avg_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }
                        </div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total TP ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value tp-col">{ price_unit.display_amount(total_tp_amount) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ format!("Total SL ({})", price_unit.unit_label()) }</div>
                        <div class="sim-summary-value sl-col">{ price_unit.display_amount(total_sl_amount) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Avg Hold" }</div>
                        <div class="sim-summary-value">{ avg_holding_secs.map(|s| format_age(s as i64)).unwrap_or_else(|| "—".into()) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Best" }</div>
                        <div class="sim-summary-value tp-col">{ best_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }</div>
                    </div>
                    <div class="sim-summary-stat">
                        <div class="sim-summary-label">{ "Worst" }</div>
                        <div class="sim-summary-value sl-col">{ worst_pnl_pct.map(|v| format!("{:+.1}%", v)).unwrap_or_else(|| "—".into()) }</div>
                    </div>
                </div>
            </div>
        }
    } else {
        html! {}
    };

    // ── Matched tokens panel ──────────────────────────────────────────────────
    let matched_panel = if *matched_loading {
        html! {
            <div class="sim-loading">
                <span class="sim-spinner">{ "⟳" }</span>
                { " Loading matched tokens…" }
            </div>
        }
    } else if let Some(err) = &*matched_error {
        html! { <div class="inline-error">{ err }</div> }
    } else if let Some((_, tokens)) = &*matched_result {
        let rule_name = matched_result.as_ref()
            .and_then(|(id, _)| tpsl.rules.iter().find(|r| &r.id == id).map(|r| r.rule_name.clone()))
            .unwrap_or_default();

        let on_close = {
            let matched_result = matched_result.clone();
            let matched_error = matched_error.clone();
            Callback::from(move |_: MouseEvent| {
                matched_result.set(None);
                matched_error.set(None);
            })
        };

        html! {
            <div class="sim-result">
                <div class="sim-result-header">
                    <span class="sim-result-title">
                        { "Matched Tokens — " }{ &rule_name }
                        <span class="matched-count-badge">{ tokens.len() }</span>
                    </span>
                    <button class="sim-close-btn" onclick={on_close} title="Close">{ "✕" }</button>
                </div>
                if tokens.is_empty() {
                    <div class="matched-empty">{ "No tokens in the database match this rule's entry criteria." }</div>
                } else {
                    <DataTable<MatchedTokenRecord>
                        columns={matched_cols.clone()}
                        rows={tokens.clone()}
                        row_key={Rc::new(|r: &MatchedTokenRecord| r.mint.clone()) as Rc<dyn Fn(&MatchedTokenRecord) -> String>}
                        default_page_size={20}
                        page_size_options={vec![20usize, 50, 100]}
                        searchable={true}
                        col_filters={true}
                        col_toggle={false}
                        item_label="tokens"
                    />
                }
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
                    <DataTable<SimulatedTokenResult>
                        columns={sim_cols.clone()}
                        rows={result.tokens.clone()}
                        row_key={Rc::new(|r: &SimulatedTokenResult| r.mint.clone()) as Rc<dyn Fn(&SimulatedTokenResult) -> String>}
                        default_page_size={20}
                        page_size_options={vec![20usize, 50, 100]}
                        searchable={true}
                        col_filters={true}
                        col_toggle={false}
                        item_label="tokens"
                    />
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
    macro_rules! onselect {
        ($field:expr) => {{
            let field = $field.clone();
            Callback::from(move |e: Event| {
                let el: web_sys::HtmlSelectElement = e.target_unchecked_into();
                field.set(el.value());
            })
        }};
    }

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">

                { sim_summary_card }

                <div class="strat-header">
                    <div class="strat-title-row">
                        <h2 class="section-title">{ "TPSL Strategies" }</h2>
                    </div>
                    <button class="add-rule-btn" onclick={open_add}>{ "+ Add Rule" }</button>
                </div>

                if tpsl.loading {
                    <div class="strat-state-msg">{ "Loading rules…" }</div>
                } else if let Some(err) = &tpsl.error {
                    <div class="inline-error">{ err }</div>
                } else {
                    <DataTable<RuleRecord>
                        columns={rules_cols}
                        rows={tpsl.rules.clone()}
                        row_key={Rc::new(|r: &RuleRecord| r.id.clone()) as Rc<dyn Fn(&RuleRecord) -> String>}
                        row_actions={Some(rules_actions)}
                        on_select={Some({
                            let selected_rule_id = selected_rule_id.clone();
                            Callback::from(move |key: Option<String>| selected_rule_id.set(key))
                        })}
                        selected_key={(*selected_rule_id).clone()}
                        default_page_size={10}
                        page_size_options={vec![10usize, 25, 50]}
                        searchable={true}
                        col_filters={true}
                        col_toggle={true}
                        item_label="rules"
                        empty_message="No rules found"
                    />
                }

                {{
                    let selected_rule = selected_rule_id.as_ref().and_then(|id| tpsl.rules.iter().find(|r| &r.id == id));
                    if let Some(_) = selected_rule {
                        if tpsl.positions_loading {
                            html! { <div class="strat-state-msg" style="margin-top:16px;">{ "Loading positions…" }</div> }
                        } else if let Some(err) = &tpsl.positions_error {
                            html! { <div class="inline-error">{ err }</div> }
                        } else {
                            html! {
                                <DataTable<RulePositionRecord>
                                    columns={positions_cols}
                                    rows={tpsl.positions.clone()}
                                    row_key={Rc::new(|r: &RulePositionRecord| r.id.clone()) as Rc<dyn Fn(&RulePositionRecord) -> String>}
                                    default_page_size={20}
                                    page_size_options={vec![20usize, 50, 100]}
                                    searchable={false}
                                    col_filters={true}
                                    col_toggle={true}
                                    item_label="positions"
                                    empty_message="No positions for this rule."
                                />
                            }
                        }
                    } else { html! {} }
                }}

                { matched_panel }

                { sim_panel }

                <Modal
                    title={modal_title.to_string()}
                    visible={modal_visible}
                    on_close={close_modal}
                    key={
                        match &*modal_mode {
                            ModalMode::Edit(rule) => rule.id.clone(),
                            _ => "modal-add".to_string(),
                        }
                    }
                >
                    <div class="rule-form">
                        <div class="rule-form-grid">
                            <label class="form-field">
                                <span class="form-label" style="color:var(--primary)">{ "Mode" }</span>
                                <select class="form-input" value={(*f_trade_mode).clone()} onchange={onselect!(f_trade_mode)}>
                                    <option value="paper" selected={*f_trade_mode == "paper"}>{ "Paper Test" }</option>
                                    <option value="real" selected={*f_trade_mode == "real"}>{ "Real Trading" }</option>
                                </select>
                                <div class="form-hint">{ "Choose 'Paper Test' for simulation only, or 'Real Trading' to enable live trades." }</div>
                            </label>

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
                                <span class="form-label" style="color:var(--text-dim)">{ "CU Limit" }<span class="form-opt">{ " opt" }</span></span>
                                <input type="number"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_limit).clone()} oninput={oninput!(f_cu_limit)}
                                    placeholder="e.g. 200000" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "CU Price" }<span class="form-opt">{ " opt" }</span></span>
                                <input type="number"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_cu_price).clone()} oninput={oninput!(f_cu_price)}
                                    placeholder="e.g. 1000000" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Max SOL Cost" }<span class="form-opt">{ " opt" }</span></span>
                                <input type="number" step="0.001"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_max_sol_cost).clone()} oninput={oninput!(f_max_sol_cost)}
                                    placeholder="0.5" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Spendable SOL In" }<span class="form-opt">{ " opt" }</span></span>
                                <input type="number" step="0.001"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_spendable_sol_in).clone()} oninput={oninput!(f_spendable_sol_in)}
                                    placeholder="1.0" readonly={is_edit && !*f_allow_edit_params} />
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Max Holding Tokens" }<span class="form-opt">{ " opt" }</span></span>
                                <input type="number" step="1"
                                    class={if is_edit && !*f_allow_edit_params { "form-input form-input-locked" } else { "form-input" }}
                                    value={(*f_max_holding_tokens).clone()} oninput={oninput!(f_max_holding_tokens)}
                                    placeholder="5" readonly={is_edit && !*f_allow_edit_params} />
                                <div class="form-hint">{ "Optional limit on how many matched tokens may be held simultaneously by this rule. This limit applies to live strategy execution and rule simulation." }</div>
                            </label>

                            <label class="form-field">
                                <span class="form-label" style="color:var(--text-dim)">{ "Total Max Trade Tokens" }<span class="form-opt">{ " opt" }</span></span>
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
                                                        html! { <><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0110 0v4" /></> }
                                                    } else {
                                                        html! { <><rect x="3" y="11" width="18" height="11" rx="2" /><path d="M7 11V7a5 5 0 0110 0v4" /><line x1="9" y1="15" x2="9" y2="18" /><line x1="15" y1="15" x2="15" y2="18" /></> }
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
                                <span class="form-hint">{ "Paste comma-separated labels or a JSON-style array. Click the icon above to populate a sample list." }</span>
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
                            <p class="form-hint">{ "Entry criteria (Init Buy, CU Limit, CU Price, Labels) are locked after creation." }</p>
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
