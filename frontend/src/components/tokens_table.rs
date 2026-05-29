use std::rc::Rc;
use yew::prelude::*;

use crate::components::data_table::{Column, SortKey};
use crate::services::api::TokenRecord;
use crate::state::PriceUnitContext;
use crate::utils::date::format_iso;
use crate::utils::format::{
    age_class, format_age, format_compact, format_decimal, format_decimal_trim, format_with_commas,
    truncate,
};

// ── Cell-style helpers (replicate TokenRow logic) ────────────────────────────

fn price_class(price: Option<f64>) -> &'static str {
    match price {
        Some(v) if v != 0.0 => {
            let abs = v.abs();
            if abs >= 1.0 { "price-normal" }
            else if abs >= 1e-3 { "price-e-3" }
            else if abs >= 1e-6 { "price-e-6" }
            else if abs >= 1e-9 { "price-e-9" }
            else if abs >= 1e-12 { "price-e-12" }
            else if abs >= 1e-15 { "price-e-15" }
            else { "price-e-smaller" }
        }
        _ => "price-normal",
    }
}

fn ratio_class(mult: Option<f64>) -> &'static str {
    match mult {
        Some(v) if v >= 100.0 => "ratio-moon",
        Some(v) if v >= 30.0  => "ratio-high",
        Some(v) if v >= 10.0  => "ratio-good",
        Some(v) if v >= 3.0   => "ratio-mid",
        Some(v) if v >= 1.5   => "ratio-low",
        _ => "ratio-flat",
    }
}

fn fep(r: &TokenRecord) -> Option<f64> {
    r.initial_buy_sol
        .and_then(|buy| r.initial_supply_token.map(|s| (buy, s)))
        .and_then(|(buy, s)| if s > 0 { Some(buy / s as f64) } else { None })
}

/// Returns the 28-column definition vec for the tokens table.
/// `price_unit` is captured by clone into render closures that need it.
pub fn token_columns(price_unit: PriceUnitContext) -> Vec<Column<TokenRecord>> {
    let pu = price_unit;
    vec![
        // ── Identity ─────────────────────────────────────────────────────────
        Column {
            key: "symbol", label: "Symbol",
            render: Rc::new(|r: &TokenRecord| {
                let sym = if r.symbol.is_empty() { truncate(&r.mint_address, 8) } else { r.symbol.clone() };
                html! { <a href={format!("https://gmgn.ai/sol/token/{}", r.mint_address)} target="_blank" rel="noreferrer" class="symbol-link-inline">{ sym }</a> }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.symbol.clone()))),
            search_value: Rc::new(|r: &TokenRecord| format!("{} {} {}", r.symbol, r.name, r.mint_address)),
            cell_class: None, sortable: true, default_visible: true, width: Some("90px"),
        },
        Column {
            key: "name", label: "Name",
            render: Rc::new(|r: &TokenRecord| html! { r.name.clone() }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.name.clone()))),
            search_value: Rc::new(|r: &TokenRecord| r.name.clone()),
            cell_class: None, sortable: true, default_visible: true, width: Some("120px"),
        },
        Column {
            key: "mint", label: "Mint",
            render: Rc::new(|r: &TokenRecord| {
                let s = truncate(&r.mint_address, 10);
                html! { <a href={format!("https://solscan.io/token/{}", r.mint_address)} target="_blank" rel="noreferrer" class="addr">{ s }</a> }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.mint_address.clone()))),
            search_value: Rc::new(|r: &TokenRecord| r.mint_address.clone()),
            cell_class: None, sortable: true, default_visible: true, width: Some("130px"),
        },
        Column {
            key: "creator", label: "Creator",
            render: Rc::new(|r: &TokenRecord| {
                let s = truncate(&r.creator_address, 10);
                html! { <a href={format!("https://solscan.io/account/{}", r.creator_address)} target="_blank" rel="noreferrer" class="addr">{ s }</a> }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.creator_address.clone()))),
            search_value: Rc::new(|r: &TokenRecord| r.creator_address.clone()),
            cell_class: None, sortable: true, default_visible: true, width: Some("130px"),
        },
        Column {
            key: "create_tx", label: "Create TX",
            render: Rc::new(|r: &TokenRecord| {
                let s = truncate(&r.create_tx_address, 10);
                html! { <a href={format!("https://solscan.io/tx/{}", r.create_tx_address)} target="_blank" rel="noreferrer" class="addr">{ s }</a> }
            }),
            sort_value: None,
            search_value: Rc::new(|r: &TokenRecord| r.create_tx_address.clone()),
            cell_class: None, sortable: false, default_visible: true, width: Some("130px"),
        },
        // ── Lifecycle ─────────────────────────────────────────────────────────
        Column {
            key: "age", label: "Age",
            render: Rc::new(|r: &TokenRecord| {
                let cls = age_class(r.age);
                html! { <span class={cls}>{ format_age(r.age) }</span> }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Num(r.age as f64))),
            search_value: Rc::new(|r: &TokenRecord| format_age(r.age)),
            cell_class: None, sortable: true, default_visible: true, width: Some("72px"),
        },
        Column {
            key: "created", label: "Created",
            render: Rc::new(|r: &TokenRecord| html! { format_iso(&r.created_at) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.created_at.clone()))),
            search_value: Rc::new(|r: &TokenRecord| r.created_at.clone()),
            cell_class: None, sortable: true, default_visible: true, width: Some("110px"),
        },
        // ── Activity ──────────────────────────────────────────────────────────
        Column {
            key: "last_trade", label: "Last Trade",
            render: Rc::new(|r: &TokenRecord| html! { r.last_trade_at.as_deref().map(format_iso).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.last_trade_at.as_ref().map(|s| SortKey::Str(s.clone())).unwrap_or(SortKey::Nothing))),
            search_value: Rc::new(|r: &TokenRecord| r.last_trade_at.clone().unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("110px"),
        },
        Column {
            key: "trade_count", label: "Trades",
            render: Rc::new(|r: &TokenRecord| html! { r.trade_count.to_string() }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Num(r.trade_count as f64))),
            search_value: Rc::new(|r: &TokenRecord| r.trade_count.to_string()),
            cell_class: None, sortable: true, default_visible: true, width: Some("66px"),
        },
        // ── ATH ───────────────────────────────────────────────────────────────
        Column {
            key: "ath_price", label: "ATH",
            render: { let p = pu.clone(); Rc::new(move |r: &TokenRecord| html! { r.ath_price.map(|v| p.display_price(v)).unwrap_or_else(|| "-".into()) }) },
            sort_value: Some(Rc::new(|r: &TokenRecord| r.ath_price.map_or(SortKey::Nothing, SortKey::Num))),
            search_value: Rc::new(|r: &TokenRecord| r.ath_price.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("88px"),
        },
        Column {
            key: "ath_timestamp", label: "ATH At",
            render: Rc::new(|r: &TokenRecord| html! { r.ath_timestamp.as_deref().map(format_iso).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.ath_timestamp.as_ref().map(|s| SortKey::Str(s.clone())).unwrap_or(SortKey::Nothing))),
            search_value: Rc::new(|r: &TokenRecord| r.ath_timestamp.clone().unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("110px"),
        },
        Column {
            key: "ath_fep_ratio", label: "ATH/FEP",
            render: Rc::new(|r: &TokenRecord| {
                let ratio = fep(r).and_then(|f| r.ath_price.and_then(|ath| if f != 0.0 { Some(ath / f) } else { None }));
                match ratio {
                    Some(v) => { let cls = ratio_class(Some(v)); html! { <span class={cls}>{ format!("{}x", format_decimal_trim(v, 2)) }</span> } }
                    None => html! { { "-" } },
                }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| {
                fep(r).and_then(|f| r.ath_price.and_then(|ath| if f != 0.0 { Some(ath / f) } else { None })).map_or(SortKey::Nothing, SortKey::Num)
            })),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("88px"),
        },
        // ── Price ─────────────────────────────────────────────────────────────
        Column {
            key: "current_price", label: "Price",
            render: { let p = pu.clone(); Rc::new(move |r: &TokenRecord| {
                let cls = price_class(r.current_price);
                html! { <span class={cls}>{ r.current_price.map(|v| p.display_price(v)).unwrap_or_else(|| "-".into()) }</span> }
            }) },
            sort_value: Some(Rc::new(|r: &TokenRecord| r.current_price.map_or(SortKey::Nothing, SortKey::Num))),
            search_value: Rc::new(|r: &TokenRecord| r.current_price.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("88px"),
        },
        Column {
            key: "current_fep_ratio", label: "Cur/FEP",
            render: Rc::new(|r: &TokenRecord| {
                let ratio = fep(r).and_then(|f| r.current_price.and_then(|cur| if f != 0.0 { Some(cur / f) } else { None }));
                match ratio {
                    Some(v) => { let cls = ratio_class(Some(v)); html! { <span class={cls}>{ format!("{}x", format_decimal_trim(v, 2)) }</span> } }
                    None => html! { { "-" } },
                }
            }),
            sort_value: Some(Rc::new(|r: &TokenRecord| {
                fep(r).and_then(|f| r.current_price.and_then(|cur| if f != 0.0 { Some(cur / f) } else { None })).map_or(SortKey::Nothing, SortKey::Num)
            })),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("76px"),
        },
        // ── Market ────────────────────────────────────────────────────────────
        Column {
            key: "market_cap", label: "MCap",
            render: { let p = pu.clone(); Rc::new(move |r: &TokenRecord| html! { r.market_cap.map(|v| p.display_compact(v, 3)).unwrap_or_else(|| "-".into()) }) },
            sort_value: Some(Rc::new(|r: &TokenRecord| r.market_cap.map_or(SortKey::Nothing, SortKey::Num))),
            search_value: Rc::new(|r: &TokenRecord| r.market_cap.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("84px"),
        },
        Column {
            key: "volume", label: "Volume",
            render: { let p = pu.clone(); Rc::new(move |r: &TokenRecord| html! { p.display_compact(r.volume_sol_total, 4) }) },
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Num(r.volume_sol_total))),
            search_value: Rc::new(|r: &TokenRecord| r.volume_sol_total.to_string()),
            cell_class: None, sortable: true, default_visible: true, width: Some("78px"),
        },
        // ── Buy / Supply ──────────────────────────────────────────────────────
        Column {
            key: "initial_buy", label: "Init Buy",
            render: { let p = pu.clone(); Rc::new(move |r: &TokenRecord| html! { r.initial_buy_sol.map(|v| p.display_amount(v)).unwrap_or_else(|| "-".into()) }) },
            sort_value: Some(Rc::new(|r: &TokenRecord| r.initial_buy_sol.map_or(SortKey::Nothing, SortKey::Num))),
            search_value: Rc::new(|r: &TokenRecord| r.initial_buy_sol.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("78px"),
        },
        Column {
            key: "init_supply", label: "Init Supply",
            render: Rc::new(|r: &TokenRecord| html! { r.initial_supply_token.map(|v| format_compact(v as f64, 2)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.initial_supply_token.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|r: &TokenRecord| r.initial_supply_token.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("90px"),
        },
        // ── Cost ──────────────────────────────────────────────────────────────
        Column {
            key: "token_amount", label: "Token Amt",
            render: Rc::new(|r: &TokenRecord| html! { r.token_amount.map(|v| format_compact(v as f64, 2)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.token_amount.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("90px"),
        },
        Column {
            key: "max_sol_cost", label: "Max SOL Cost",
            render: Rc::new(|r: &TokenRecord| html! { r.max_sol_cost.map(|v| format_decimal_trim(v as f64 / 1_000_000_000.0, 3)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.max_sol_cost.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("100px"),
        },
        // ── Liquidity ─────────────────────────────────────────────────────────
        Column {
            key: "spendable_sol_in", label: "Spendable SOL In",
            render: Rc::new(|r: &TokenRecord| html! { r.spendable_sol_in.map(|v| format_decimal_trim(v as f64 / 1_000_000_000.0, 3)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.spendable_sol_in.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("100px"),
        },
        Column {
            key: "min_tokens_out", label: "Min Tokens",
            render: Rc::new(|r: &TokenRecord| html! { r.min_tokens_out.map(|v| format_compact(v as f64, 2)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.min_tokens_out.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|_| String::new()),
            cell_class: None, sortable: true, default_visible: true, width: Some("90px"),
        },
        // ── Technical ─────────────────────────────────────────────────────────
        Column {
            key: "cu_limit", label: "CU Limit",
            render: Rc::new(|r: &TokenRecord| html! { r.cu_limit.map(|v| v.to_string()).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.cu_limit.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|r: &TokenRecord| r.cu_limit.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("72px"),
        },
        Column {
            key: "cu_price", label: "CU Price",
            render: Rc::new(|r: &TokenRecord| html! { r.cu_price.map(|v| format_with_commas(v)).unwrap_or_else(|| "-".into()) }),
            sort_value: Some(Rc::new(|r: &TokenRecord| r.cu_price.map_or(SortKey::Nothing, |v| SortKey::Num(v as f64)))),
            search_value: Rc::new(|r: &TokenRecord| r.cu_price.map(|v| v.to_string()).unwrap_or_default()),
            cell_class: None, sortable: true, default_visible: true, width: Some("72px"),
        },
        Column {
            key: "ix_count", label: "IX Count",
            render: Rc::new(|r: &TokenRecord| html! { r.ix_labels_count.to_string() }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Num(r.ix_labels_count as f64))),
            search_value: Rc::new(|r: &TokenRecord| r.ix_labels_count.to_string()),
            cell_class: None, sortable: true, default_visible: true, width: Some("54px"),
        },
        Column {
            key: "ix_labels", label: "IX Labels",
            render: Rc::new(|r: &TokenRecord| {
                let s = r.instruction_labels.as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "-".into());
                html! { <span title={s.clone()} class="labels-col">{ s }</span> }
            }),
            sort_value: None,
            search_value: Rc::new(|r: &TokenRecord| {
                r.instruction_labels.as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default()
            }),
            cell_class: None, sortable: false, default_visible: true, width: Some("180px"),
        },
        // ── Status ────────────────────────────────────────────────────────────
        Column {
            key: "migrated", label: "Migrated",
            render: Rc::new(|r: &TokenRecord| html! { { if r.is_migrated { "✓" } else { "" } } }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.is_migrated.to_string()))),
            search_value: Rc::new(|r: &TokenRecord| r.is_migrated.to_string()),
            cell_class: None, sortable: true, default_visible: true, width: Some("66px"),
        },
        Column {
            key: "mayhem_mode", label: "Mayhem",
            render: Rc::new(|r: &TokenRecord| html! { { if r.is_mayhem_mode { "✓" } else { "" } } }),
            sort_value: Some(Rc::new(|r: &TokenRecord| SortKey::Str(r.is_mayhem_mode.to_string()))),
            search_value: Rc::new(|r: &TokenRecord| r.is_mayhem_mode.to_string()),
            cell_class: None, sortable: true, default_visible: true, width: Some("66px"),
        },
    ]
}
