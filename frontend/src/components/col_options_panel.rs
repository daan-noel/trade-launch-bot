use std::collections::HashSet;
use yew::prelude::*;

/// (sort_key, display_label, col_width_px, optional_th_class)
pub const COLUMNS: &[(&str, &str, u32, Option<&str>)] = &[
    // Identity
    ("symbol", "Symbol", 90, None),
    ("name", "Name", 120, None),
    ("mint", "Mint", 130, None),
    ("creator", "Creator", 130, None),
    ("create_tx", "Create TX", 130, None),
    // Lifecycle
    ("age", "Age", 72, None),
    ("created", "Created", 110, None),
    // Activity
    ("last_trade", "Last Trade", 110, None),
    ("trade_count", "Trades", 66, None),
    // ATH
    ("ath_price", "ATH", 88, None),
    ("ath_timestamp", "ATH At", 110, None),
    ("ath_fep_ratio", "ATH/FEP", 88, Some("th-ath-fep")),
    // Price
    ("current_price", "Price", 88, None),
    ("current_fep_ratio", "Cur/FEP", 76, Some("th-cur-fep")),
    // Market
    ("market_cap", "MCap", 84, None),
    ("volume", "Volume", 78, None),
    // Buy / Supply
    ("initial_buy", "Init Buy", 78, None),
    ("init_supply", "Init Supply", 90, None),
    // Cost
    ("token_amount", "Token Amt", 90, None),
    ("max_sol_cost", "Max SOL Cost", 100, Some("th-cost")),
    // Liquidity
    (
        "spendable_sol_in",
        "Spendable SOL In",
        100,
        Some("th-liquidity"),
    ),
    ("min_tokens_out", "Min Tokens", 90, None),
    // Technical
    ("cu_limit", "CU Limit", 72, None),
    ("cu_price", "CU Price", 72, None),
    ("ix_count", "IX Count", 54, None),
    ("ix_labels", "IX Labels", 180, None),
    // Status
    ("migrated", "Migrated", 66, None),
    ("mayhem_mode", "Mayhem", 66, None),
];

pub const COLUMN_GROUPS: &[(&str, &[&str])] = &[
    (
        "Identity",
        &["symbol", "name", "mint", "creator", "create_tx"],
    ),
    ("Lifecycle", &["age", "created"]),
    ("Activity", &["last_trade", "trade_count"]),
    ("ATH", &["ath_price", "ath_timestamp", "ath_fep_ratio"]),
    ("Price", &["current_price", "current_fep_ratio"]),
    ("Market", &["market_cap", "volume"]),
    ("Buy / Supply", &["initial_buy", "init_supply"]),
    ("Cost", &["token_amount", "max_sol_cost"]),
    ("Liquidity", &["spendable_sol_in", "min_tokens_out"]),
    (
        "Technical",
        &["cu_limit", "cu_price", "ix_count", "ix_labels"],
    ),
    ("Status", &["migrated", "mayhem_mode"]),
];

pub fn column_group(key: &str) -> Option<&'static str> {
    for (group, keys) in COLUMN_GROUPS.iter() {
        if keys.contains(&key) {
            return Some(*group);
        }
    }
    None
}

pub fn compute_group_boundaries(vis: &[bool]) -> Vec<bool> {
    let mut boundaries = Vec::with_capacity(vis.len());
    let mut prev_group: Option<&str> = None;

    for (i, &(key, _, _, _)) in COLUMNS.iter().enumerate() {
        let visible = vis.get(i).copied().unwrap_or(false);
        let group = column_group(key);

        if visible {
            let boundary = match (prev_group, group) {
                (Some(prev), Some(curr)) => prev != curr,
                _ => false,
            };
            boundaries.push(boundary);
            prev_group = group;
        } else {
            boundaries.push(false);
        }
    }

    boundaries
}

// ── Component ─────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct ColOptionsPanelProps {
    pub visible_cols: HashSet<String>,
    pub on_toggle_col: Callback<String>,
}

#[function_component(ColOptionsPanel)]
pub fn col_options_panel(props: &ColOptionsPanelProps) -> Html {
    let on_toggle_col = props.on_toggle_col.clone();
    html! {
        <div class="col-options-panel">
            <div class="col-options-header">{ "COLUMN OPTIONS" }</div>
            <div class="col-options-groups">
                { for COLUMN_GROUPS.iter().map(|(group_label, keys)| {
                    html! {
                        <div class="col-opt-group">
                            <div class="col-opt-group-title">{ *group_label }</div>
                            <div class="col-opt-group-body">
                                { for keys.iter().filter_map(|key| {
                                    COLUMNS.iter().find(|&&(k, _, _, _)| k == *key).map(|&(k, label, _, _)| {
                                        let checked = props.visible_cols.contains(k);
                                        let key_str = k.to_string();
                                        let on_change = {
                                            let cb = on_toggle_col.clone();
                                            Callback::from(move |_: Event| cb.emit(key_str.clone()))
                                        };
                                        html! {
                                            <label class="col-opt-item">
                                                <input type="checkbox" checked={checked} onchange={on_change} />
                                                <span>{ label }</span>
                                            </label>
                                        }
                                    })
                                }) }
                            </div>
                        </div>
                    }
                }) }
            </div>
        </div>
    }
}
