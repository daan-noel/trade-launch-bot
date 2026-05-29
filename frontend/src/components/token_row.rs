use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::stat_card::{AddrCard, StatCard, StatVariant};
use crate::services::api::{TokenDetailRecord, TokenRecord};
use crate::state::PriceUnitContext;
use crate::utils::date::format_iso;
use crate::utils::format::{
    age_class, format_age, format_compact, format_decimal, format_decimal_trim, format_price,
    format_with_commas, truncate,
};
use serde_json::Value;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn price_class(price: Option<f64>) -> &'static str {
    match price {
        Some(v) if v != 0.0 => {
            let abs = v.abs();
            if abs >= 1.0 {
                "price-normal"
            } else if abs >= 1e-3 {
                "price-e-3"
            } else if abs >= 1e-6 {
                "price-e-6"
            } else if abs >= 1e-9 {
                "price-e-9"
            } else if abs >= 1e-12 {
                "price-e-12"
            } else if abs >= 1e-15 {
                "price-e-15"
            } else {
                "price-e-smaller"
            }
        }
        _ => "price-normal",
    }
}

/// CSS class for ratio cells -- `mult` is the price multiple (e.g. 10 = 10x ATH vs entry).
fn ratio_class(mult: Option<f64>) -> &'static str {
    match mult {
        Some(v) if v >= 100.0 => "ratio-moon",
        Some(v) if v >= 30.0 => "ratio-high",
        Some(v) if v >= 10.0 => "ratio-good",
        Some(v) if v >= 3.0 => "ratio-mid",
        Some(v) if v >= 1.5 => "ratio-low",
        _ => "ratio-flat",
    }
}

/// Maps a price multiple to a `StatVariant` for detail-panel cards.
fn ratio_variant(mult: Option<f64>) -> StatVariant {
    match mult {
        Some(v) if v >= 100.0 => StatVariant::Danger,
        Some(v) if v >= 30.0 => StatVariant::Accent,
        Some(v) if v >= 10.0 => StatVariant::Warning,
        Some(v) if v >= 3.0 => StatVariant::Primary,
        Some(v) if v >= 1.5 => StatVariant::Info,
        _ => StatVariant::Muted,
    }
}

// ── Props ─────────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct Props {
    pub token: TokenRecord,
    pub selected: bool,
    pub detail: Option<TokenDetailRecord>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    pub on_select: Callback<String>,
    #[prop_or_default]
    pub row_num: Option<usize>,
    /// Visibility mask — one bool per `COLUMNS` entry, in the same order.
    /// Defaults to all-visible when not supplied.
    #[prop_or_default]
    pub visible_cols: Vec<bool>,
    #[prop_or_default]
    pub group_borders: Vec<bool>,
    /// Which rendered column position (1-based) is currently hovered, if any.
    #[prop_or_default]
    pub hovered_column: Option<usize>,
    /// Callback to notify the parent which column (rendered pos) the mouse entered.
    #[prop_or_default]
    pub on_hover_column: Callback<Option<usize>>,
}

// ── Update-flash tracking ─────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
struct UpdateFlags {
    current_price: bool,
    ath_price: bool,
    ath_timestamp: bool,
    volume_sol_total: bool,
    market_cap: bool,
    ath_fep_ratio: bool,
    current_fep_ratio: bool,
    trade_count: bool,
}

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component(TokenRow)]
pub fn token_row(props: &Props) -> Html {
    let s = &props.token;

    // ── Derived display values ────────────────────────────────────────────────
    let display_symbol = if s.symbol.is_empty() {
        truncate(&s.mint_address, 8)
    } else {
        s.symbol.clone()
    };

    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above TokenRow");
    let current_price_value = s.current_price;
    let current_price = current_price_value
        .map(|v| price_unit.display_price(v))
        .unwrap_or_else(|| "-".into());

    let first_entry_price_value = s
        .initial_buy_sol
        .and_then(|buy| s.initial_supply_token.map(|supply| (buy, supply)))
        .and_then(|(buy, supply)| {
            if supply > 0 {
                Some(buy / supply as f64)
            } else {
                None
            }
        });

    // ath_fep_ratio_value: (ath / fep) * 100  ->  price multiple x 100
    let ath_fep_ratio_value = first_entry_price_value.and_then(|fep| {
        s.ath_price.and_then(|ath| {
            if fep != 0.0 {
                Some((ath / fep) * 100.0)
            } else {
                None
            }
        })
    });
    let ath_fep_mult = ath_fep_ratio_value.map(|v| v / 100.0);
    let ath_fep_display = ath_fep_mult
        .map(|v| format!("{}x", format_decimal_trim(v, 2)))
        .unwrap_or_else(|| "-".into());
    let ath_fep_pct = ath_fep_ratio_value
        .map(|v| format!("{}%", format_decimal(v, 1)))
        .unwrap_or_else(|| "-".into());

    let current_fep_ratio_value = first_entry_price_value.and_then(|fep| {
        current_price_value.and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
    });
    let cur_fep_display = current_fep_ratio_value
        .map(|v| format!("{}x", format_decimal_trim(v, 2)))
        .unwrap_or_else(|| "-".into());

    let ath_price_str = s
        .ath_price
        .map(|v| price_unit.display_price(v))
        .unwrap_or_else(|| "-".into());
    let market_cap = s
        .market_cap
        .map(|v| price_unit.display_compact(v, 3))
        .unwrap_or_else(|| "-".into());
    let initial_buy = s
        .initial_buy_sol
        .map(|v| price_unit.display_amount(v))
        .unwrap_or_else(|| "-".into());
    let cu_price = s
        .cu_price
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".to_string());

    let age_text = format_age(s.age);
    let age_cls = age_class(s.age);

    // ── Click handler ─────────────────────────────────────────────────────────
    let onclick = {
        let on_select = props.on_select.clone();
        let mint = s.mint_address.clone();
        Callback::from(move |_: MouseEvent| on_select.emit(mint.clone()))
    };

    let detail_copy_copied = use_state(|| false);

    // ── Detail panel ──────────────────────────────────────────────────────────
    let detail_panel = if props.selected {
        if props.detail_loading {
            html! {
                <div class="detail-loading">
                    <span style="color:var(--text-dim); font-size:12px;">{ "Loading details..." }</span>
                </div>
            }
        } else if let Some(err) = &props.detail_error {
            html! {
                <p class="error" style="padding:12px;">{ err }</p>
            }
        } else if let Some(detail) = &props.detail {
            // ── Compute detail-specific values ────────────────────────────
            let d_fep = detail
                .initial_buy_sol
                .and_then(|buy| detail.initial_supply_token.map(|s| (buy, s)))
                .and_then(|(buy, supply)| {
                    if supply > 0 {
                        Some(buy / supply as f64)
                    } else {
                        None
                    }
                });
            let d_fep_str = d_fep.map(format_price).unwrap_or_else(|| "-".into());

            let d_ath_mult = d_fep.and_then(|fep| {
                detail
                    .ath_price
                    .and_then(|ath| if fep != 0.0 { Some(ath / fep) } else { None })
            });
            let d_ath_pct = d_fep.and_then(|fep| {
                detail.ath_price.and_then(|ath| {
                    if fep != 0.0 {
                        Some((ath / fep) * 100.0)
                    } else {
                        None
                    }
                })
            });
            let d_ath_mult_str = d_ath_mult
                .map(|v| {
                    format!(
                        "{}x  ({}%)",
                        format_decimal_trim(v, 2),
                        format_decimal(d_ath_pct.unwrap_or(0.0), 1)
                    )
                })
                .unwrap_or_else(|| "-".into());

            let d_cur_mult = d_fep.and_then(|fep| {
                detail
                    .current_price
                    .and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
            });
            let d_cur_pct = d_fep.and_then(|fep| {
                detail.current_price.and_then(|cur| {
                    if fep != 0.0 {
                        Some((cur / fep) * 100.0)
                    } else {
                        None
                    }
                })
            });
            let d_cur_mult_str = d_cur_mult
                .map(|v| {
                    format!(
                        "{}x  ({}%)",
                        format_decimal_trim(v, 2),
                        format_decimal(d_cur_pct.unwrap_or(0.0), 1)
                    )
                })
                .unwrap_or_else(|| "-".into());

            let d_ath_str = detail
                .ath_price
                .map(|v| price_unit.display_price(v))
                .unwrap_or_else(|| "-".into());
            let d_cur_str = detail
                .current_price
                .map(|v| price_unit.display_price(v))
                .unwrap_or_else(|| "-".into());
            let d_ath_ts = detail
                .ath_timestamp
                .as_deref()
                .map(format_iso)
                .unwrap_or_else(|| "-".into());
            let d_last_trade = detail
                .last_trade_at
                .as_deref()
                .map(format_iso)
                .unwrap_or_else(|| "-".into());
            let d_volume = detail
                .volume_sol_total
                .map(|v| price_unit.display_compact(v, 4))
                .unwrap_or_else(|| "-".into());
            let d_mcap = detail
                .market_cap
                .map(|v| price_unit.display_compact(v, 4))
                .unwrap_or_else(|| "-".into());
            let d_trades = detail
                .trade_count
                .map_or_else(|| "-".into(), |v| v.to_string());
            let d_wallets = detail
                .unique_wallets_in_window
                .map_or_else(|| "-".into(), |v| v.to_string());
            let d_init_buy = detail
                .initial_buy_sol
                .map(|v| price_unit.display_amount(v))
                .unwrap_or_else(|| "-".into());
            let d_init_supply = detail
                .initial_supply_token
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into());
            let d_cu_limit = detail
                .cu_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into());
            let d_cu_price = detail
                .cu_price
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into());
            let d_label_count = detail
                .instruction_labels
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            let d_created = format_iso(&detail.created_at);
            let d_status = if detail.is_migrated {
                "Migrated ✓"
            } else {
                "Bonding Curve"
            };
            let d_status_cls = if detail.is_migrated {
                "detail-status-migrated"
            } else {
                "detail-status-bonding"
            };

            let d_symbol = if detail.symbol.is_empty() {
                truncate(&detail.mint_address, 8)
            } else {
                detail.symbol.clone()
            };

            let creator_solscan = format!("https://solscan.io/account/{}", detail.creator_address);
            let creator_gmgn = format!("https://gmgn.ai/sol/address/{}", detail.creator_address);
            let mint_solscan = format!("https://solscan.io/token/{}", detail.mint_address);
            let mint_gmgn = format!("https://gmgn.ai/sol/token/{}", detail.mint_address);
            let create_tx_solscan = format!("https://solscan.io/tx/{}", detail.create_tx_address);

            let creator_short = truncate(&detail.creator_address, 12);
            let mint_short = truncate(&detail.mint_address, 12);
            let create_tx_short = truncate(&detail.create_tx_address, 12);
            let bonding_short = detail
                .bonding_curve_address
                .as_deref()
                .map(|a| truncate(a, 12))
                .unwrap_or_else(|| "-".into());
            let bonding_full = detail.bonding_curve_address.clone().unwrap_or_default();
            let bonding_solscan = detail
                .bonding_curve_address
                .as_ref()
                .map(|a| format!("https://solscan.io/account/{}", a));
            let bonding_gmgn = detail
                .bonding_curve_address
                .as_ref()
                .map(|a| format!("https://gmgn.ai/sol/address/{}", a));
            let bonding_html = if let Some(url) = bonding_solscan {
                html! { <AddrCard label="Bonding Curve" short={bonding_short} full={bonding_full} solscan_url={url} gmgn_url={bonding_gmgn} /> }
            } else {
                html! { <StatCard label="Bonding Curve" value="-" variant={StatVariant::Muted} /> }
            };

            let on_copy_labels = {
                let instruction_labels = detail.instruction_labels.clone();
                let copied = detail_copy_copied.clone();
                Callback::from(move |_: MouseEvent| {
                    let text = serde_json::to_string(&instruction_labels).unwrap_or_default();
                    let copied = copied.clone();
                    spawn_local(async move {
                        if let Some(win) = web_sys::window() {
                            let cb = win.navigator().clipboard();
                            let _ =
                                wasm_bindgen_futures::JsFuture::from(cb.write_text(&text)).await;
                            copied.set(true);
                            let copied_reset = copied.clone();
                            gloo::timers::callback::Timeout::new(1500, move || {
                                copied_reset.set(false)
                            })
                            .forget();
                        }
                    });
                })
            };

            let instruction_html = build_instruction_html(&detail.instruction_labels);

            html! {
                <section class="token-detail-panel">
                    <div class="detail-panel-inner">

                        <div class="detail-header">
                            <div class="detail-title-group">
                                <span class="detail-symbol">{ &d_symbol }</span>
                                <span class="detail-token-name">{ &detail.name }</span>
                            </div>
                            <span class={classes!("detail-status-badge", d_status_cls)}>
                                { d_status }
                            </span>
                        </div>

                        <div class="detail-body">
                        <div class="detail-left">

                        <div class="detail-section">
                            <div class="detail-section-title">{ "Price Performance" }</div>
                            <div class="stat-grid-3">
                                <StatCard label="First Entry Price" value={d_fep_str} large=true />
                                <StatCard label="ATH Price" value={d_ath_str} variant={StatVariant::Primary} large=true />
                                <StatCard label="Current Price" value={d_cur_str} large=true />
                                <StatCard label="ATH / FEP" value={d_ath_mult_str} variant={ratio_variant(d_ath_mult)} large=true />
                                <StatCard label="Current / FEP" value={d_cur_mult_str} variant={ratio_variant(d_cur_mult)} large=true />
                                <StatCard label="ATH Timestamp" value={d_ath_ts} variant={StatVariant::Muted} />
                            </div>
                        </div>

                        <div class="detail-section">
                            <div class="detail-section-title">{ "Activity & Market" }</div>
                            <div class="stat-grid-3">
                                <StatCard label={format!("Volume ({})", price_unit.unit_label())} value={d_volume} variant={StatVariant::Info} bold={true} />
                                <StatCard label={format!("Market Cap ({})", price_unit.unit_label())} value={d_mcap} bold={true} />
                                <StatCard label="Trade Count" value={d_trades} bold={true} />
                                <StatCard label="Unique Wallets" value={d_wallets} variant={StatVariant::Info} bold={true} />
                                <StatCard label="Last Trade" value={d_last_trade} variant={StatVariant::Muted} />
                                <StatCard label="Created" value={d_created} variant={StatVariant::Muted} />
                            </div>
                        </div>

                        <div class="detail-section">
                            <div class="detail-section-title">{ "Creation Parameters" }</div>
                            <div class="stat-grid-4">
                                <StatCard label={format!("Initial Buy ({})", price_unit.unit_label())} value={d_init_buy} />
                                <StatCard label="Initial Supply" value={d_init_supply} />
                                <StatCard label="CU Limit" value={d_cu_limit} variant={StatVariant::Muted} bold={true} />
                                <StatCard label="CU Price" value={d_cu_price} variant={StatVariant::Muted} bold={true} />
                            </div>
                        </div>

                        <div class="detail-section">
                            <div class="detail-section-title">{ "Addresses" }</div>
                            <div class="stat-grid-4">
                                <AddrCard
                                    label="Creator"
                                    short={creator_short}
                                    full={detail.creator_address.clone()}
                                    solscan_url={creator_solscan}
                                    gmgn_url={Some(creator_gmgn)}
                                />
                                <AddrCard
                                    label="Mint"
                                    short={mint_short}
                                    full={detail.mint_address.clone()}
                                    solscan_url={mint_solscan}
                                    gmgn_url={Some(mint_gmgn)}
                                />
                                <AddrCard
                                    label="Create TX"
                                    short={create_tx_short}
                                    full={detail.create_tx_address.clone()}
                                    solscan_url={create_tx_solscan}
                                />
                                { bonding_html }
                            </div>
                        </div>

                        </div>

                        <div class="detail-body-divider"></div>
                        <div class="detail-right">
                            <div class="detail-section-title">
                                <span>{ format!("Instruction Labels  ({})", d_label_count) }</span>
                                <button class={classes!("detail-copy-btn", (*detail_copy_copied).then_some("detail-copy-ok"))}
                                    onclick={on_copy_labels}
                                    title={if *detail_copy_copied { "Copied!" } else { "Copy labels to clipboard" }}>
                                    { if *detail_copy_copied {
                                        html! {
                                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M5 13l4 4L19 7" />
                                            </svg>
                                        }
                                    } else {
                                        html! {
                                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                                                <rect x="9" y="9" width="13" height="13" rx="2" />
                                                <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
                                            </svg>
                                        }
                                    } }
                                </button>
                            </div>
                            { instruction_html }
                        </div>
                        </div>

                    </div>
                </section>
            }
        } else {
            html! {
                <div class="token-detail-panel-empty">
                    <span style="color:var(--text-dim); font-size:12px;">
                        { "Select a row to load detailed info." }
                    </span>
                </div>
            }
        }
    } else {
        html! {}
    };

    // ── Update-flash effect ───────────────────────────────────────────────────
    let update_flags = use_state(|| UpdateFlags::default());
    let previous = use_mut_ref(|| None::<TokenRecord>);
    {
        let token = props.token.clone();
        let update_flags = update_flags.clone();
        let previous = previous.clone();
        use_effect_with(token, move |token| {
            let mut flags = UpdateFlags::default();
            if let Some(prev) = &*previous.borrow() {
                flags.current_price = prev.current_price != token.current_price;
                flags.ath_price = prev.ath_price != token.ath_price;
                flags.ath_timestamp = prev.ath_timestamp != token.ath_timestamp;
                flags.volume_sol_total = prev.volume_sol_total != token.volume_sol_total;
                flags.market_cap = prev.market_cap != token.market_cap;
                flags.trade_count = prev.trade_count != token.trade_count;

                let prev_fep = prev
                    .initial_buy_sol
                    .and_then(|buy| prev.initial_supply_token.map(|s| (buy, s)))
                    .and_then(|(buy, s)| if s > 0 { Some(buy / s as f64) } else { None });
                let new_fep = token
                    .initial_buy_sol
                    .and_then(|buy| token.initial_supply_token.map(|s| (buy, s)))
                    .and_then(|(buy, s)| if s > 0 { Some(buy / s as f64) } else { None });

                let prev_ath_fep = prev_fep.and_then(|fep| {
                    prev.ath_price.and_then(|ath| {
                        if fep != 0.0 {
                            Some((ath / fep) * 100.0)
                        } else {
                            None
                        }
                    })
                });
                let new_ath_fep = new_fep.and_then(|fep| {
                    token.ath_price.and_then(|ath| {
                        if fep != 0.0 {
                            Some((ath / fep) * 100.0)
                        } else {
                            None
                        }
                    })
                });
                flags.ath_fep_ratio = prev_ath_fep != new_ath_fep;

                let prev_cur_fep = prev_fep.and_then(|fep| {
                    prev.current_price
                        .and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
                });
                let new_cur_fep = new_fep.and_then(|fep| {
                    token
                        .current_price
                        .and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
                });
                flags.current_fep_ratio = prev_cur_fep != new_cur_fep;
            }
            *previous.borrow_mut() = Some(token.clone());
            update_flags.set(flags);
            || ()
        });
    }

    let row_class = if props.selected {
        classes!("selected-row")
    } else {
        classes!()
    };
    let row_num_str = props.row_num.map(|n| n.to_string()).unwrap_or_default();
    // Helper: returns true when column i should be rendered (default-show when vec not supplied)
    let show = |i: usize| props.visible_cols.get(i).copied().unwrap_or(true);

    // ── Column hover helpers ──────────────────────────────────────────────────
    // Rendered column positions: pos 0 = row-num, 1..N = visible data cols, N+1 = row-actions.
    // col_positions[i] = rendered position when show(i) is true; 0 otherwise (unused).
    let col_positions: Vec<usize> = {
        let mut pos = 1usize;
        props
            .visible_cols
            .iter()
            .map(|&vis| {
                if vis {
                    let p = pos;
                    pos += 1;
                    p
                } else {
                    0
                }
            })
            .collect()
    };
    let action_col_pos = 1 + props.visible_cols.iter().filter(|&&b| b).count();
    let hc = props.hovered_column;
    let on_hover_cb = props.on_hover_column.clone();
    // Returns a MouseEvent callback that emits the given rendered column position to the parent.
    let make_hover_cb = move |pos: usize| -> Callback<MouseEvent> {
        let cb = on_hover_cb.clone();
        Callback::from(move |_: MouseEvent| cb.emit(Some(pos)))
    };

    let border_style = |i: usize| {
        if props.group_borders.get(i).copied().unwrap_or(false) {
            "border-left: 1px solid rgba(128, 128, 128, 0.25);"
        } else {
            ""
        }
    };

    // ── Extra column display values (indices 13–21) ───────────────────────────
    let cu_limit_str = s
        .cu_limit
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let init_supply_str = s
        .initial_supply_token
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let token_amount_str = s
        .token_amount
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let max_sol_cost_str = s
        .max_sol_cost
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let spendable_sol_in_str = s
        .spendable_sol_in
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let min_tokens_out_str = s
        .min_tokens_out
        .map(|v| format_with_commas(v))
        .unwrap_or_else(|| "-".into());
    let ath_ts_str = s
        .ath_timestamp
        .as_deref()
        .map(format_iso)
        .unwrap_or_else(|| "-".into());
    let last_trade_str = s
        .last_trade_at
        .as_deref()
        .map(format_iso)
        .unwrap_or_else(|| "-".into());
    let ix_labels_str: String = s
        .instruction_labels
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".into());

    html! {
        <>
            <tr id={format!("row-{}", s.mint_address)} class={row_class}>

                <td class="row-num">{ row_num_str }</td>

                if show(0) {
                    <td class={(hc == Some(col_positions[0])).then_some("col-hover")} style={border_style(0)} onmouseenter={make_hover_cb(col_positions[0])}>
                        <div class="symbol-cell">
                            <span class="symbol-text">{ &display_symbol }</span>
                            <a
                                class="symbol-link"
                                href={format!("https://gmgn.ai/sol/token/{}", s.mint_address)}
                                target="_blank"
                                rel="noopener noreferrer"
                                title="Open on GMGN"
                            >
                                <svg width="12" height="12" viewBox="0 0 24 24" fill="none"
                                     xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
                                    <path d="M14 3h7v7" stroke="currentColor" stroke-width="2"
                                          stroke-linecap="round" stroke-linejoin="round"/>
                                    <path d="M10 14L21 3" stroke="currentColor" stroke-width="2"
                                          stroke-linecap="round" stroke-linejoin="round"/>
                                    <path d="M21 21H3V3" stroke="currentColor" stroke-width="2"
                                          stroke-linecap="round" stroke-linejoin="round"/>
                                </svg>
                            </a>
                        </div>
                    </td>
                }

                if show(1) { <td class={(hc == Some(col_positions[1])).then_some("col-hover")} style={border_style(1)} onmouseenter={make_hover_cb(col_positions[1])}>{ s.name.clone() }</td> }

                if show(2) {
                    <td class={classes!("addr", (hc == Some(col_positions[2])).then_some("col-hover"))} style={border_style(2)} onmouseenter={make_hover_cb(col_positions[2])} title={s.mint_address.clone()}>{ truncate(&s.mint_address, 12) }</td>
                }

                if show(3) {
                    <td class={classes!("addr", (hc == Some(col_positions[3])).then_some("col-hover"))} style={border_style(3)} onmouseenter={make_hover_cb(col_positions[3])} title={s.creator_address.clone()}>{ truncate(&s.creator_address, 12) }</td>
                }

                if show(4) {
                    <td class={classes!("addr", (hc == Some(col_positions[4])).then_some("col-hover"))} style={border_style(4)} onmouseenter={make_hover_cb(col_positions[4])} title={s.create_tx_address.clone()}>{ truncate(&s.create_tx_address, 12) }</td>
                }

                if show(5) {
                    <td class={classes!(age_cls, (hc == Some(col_positions[5])).then_some("col-hover"))} style={border_style(5)} onmouseenter={make_hover_cb(col_positions[5])}>
                        { age_text }
                    </td>
                }

                if show(6) {
                    <td class={(hc == Some(col_positions[6])).then_some("col-hover")} style={border_style(6)} onmouseenter={make_hover_cb(col_positions[6])}>
                        { format_iso(&s.created_at) }
                    </td>
                }

                if show(7) { <td class={classes!("dim-col", (hc == Some(col_positions[7])).then_some("col-hover"))} style={border_style(7)} onmouseenter={make_hover_cb(col_positions[7])}>{ last_trade_str.clone() }</td> }

                if show(8) {
                    <td class={classes!(update_flags.trade_count.then_some("updated-cell"), (hc == Some(col_positions[8])).then_some("col-hover"))} style={border_style(8)} onmouseenter={make_hover_cb(col_positions[8])}>
                        { format_with_commas(s.trade_count) }
                    </td>
                }

                if show(9) {
                    <td class={classes!(price_class(s.ath_price), update_flags.ath_price.then_some("updated-cell"), (hc == Some(col_positions[9])).then_some("col-hover"))} style={border_style(9)} onmouseenter={make_hover_cb(col_positions[9])}>
                        { &ath_price_str }
                    </td>
                }

                if show(10) {
                    <td class={classes!(update_flags.ath_timestamp.then_some("updated-cell"), (hc == Some(col_positions[10])).then_some("col-hover"))} style={border_style(10)} onmouseenter={make_hover_cb(col_positions[10])}>
                        { ath_ts_str.clone() }
                    </td>
                }

                if show(11) {
                    <td class={classes!(
                        "ath-fep-col",
                        ratio_class(ath_fep_mult),
                        update_flags.ath_fep_ratio.then_some("updated-cell"),
                        (hc == Some(col_positions[11])).then_some("col-hover")
                    )} style={border_style(11)} onmouseenter={make_hover_cb(col_positions[11])}>
                        <div class="ratio-cell">
                            <span class="ratio-main">{ &ath_fep_display }</span>
                            <span class="ratio-sub">{ &ath_fep_pct }</span>
                        </div>
                    </td>
                }

                if show(12) {
                    <td class={classes!(price_class(current_price_value), update_flags.current_price.then_some("updated-cell"), (hc == Some(col_positions[12])).then_some("col-hover"))} style={border_style(12)} onmouseenter={make_hover_cb(col_positions[12])}>
                        { &current_price }
                    </td>
                }

                if show(13) {
                    <td class={classes!(
                        "cur-fep-col",
                        ratio_class(current_fep_ratio_value),
                        update_flags.current_fep_ratio.then_some("updated-cell"),
                        (hc == Some(col_positions[13])).then_some("col-hover")
                    )} style={border_style(13)} onmouseenter={make_hover_cb(col_positions[13])}>
                        { &cur_fep_display }
                    </td>
                }

                if show(14) {
                    <td class={classes!(price_class(s.market_cap), update_flags.market_cap.then_some("updated-cell"), (hc == Some(col_positions[14])).then_some("col-hover"))} style={border_style(14)} onmouseenter={make_hover_cb(col_positions[14])}>
                        { &market_cap }
                    </td>
                }

                if show(15) {
                    <td class={classes!(update_flags.volume_sol_total.then_some("updated-cell"), (hc == Some(col_positions[15])).then_some("col-hover"))} style={border_style(15)} onmouseenter={make_hover_cb(col_positions[15])}>
                        { format_compact(s.volume_sol_total, 2) }
                    </td>
                }

                if show(16) {
                    <td class={(hc == Some(col_positions[16])).then_some("col-hover")} style={border_style(16)} onmouseenter={make_hover_cb(col_positions[16])}>
                        { &initial_buy }
                    </td>
                }

                if show(17) { <td class={classes!("dim-col", (hc == Some(col_positions[17])).then_some("col-hover"))} style={border_style(17)} onmouseenter={make_hover_cb(col_positions[17])}>{ init_supply_str.clone() }</td> }

                if show(18) { <td class={classes!("dim-col", (hc == Some(col_positions[18])).then_some("col-hover"))} style={border_style(18)} onmouseenter={make_hover_cb(col_positions[18])}>{ token_amount_str.clone() }</td> }

                if show(19) { <td class={classes!("dim-col", "cost-col", (hc == Some(col_positions[19])).then_some("col-hover"))} style={border_style(19)} onmouseenter={make_hover_cb(col_positions[19])}>{ max_sol_cost_str.clone() }</td> }

                if show(20) { <td class={classes!("dim-col", "liquidity-col", (hc == Some(col_positions[20])).then_some("col-hover"))} style={border_style(20)} onmouseenter={make_hover_cb(col_positions[20])}>{ spendable_sol_in_str.clone() }</td> }

                if show(21) { <td class={classes!("dim-col", (hc == Some(col_positions[21])).then_some("col-hover"))} style={border_style(21)} onmouseenter={make_hover_cb(col_positions[21])}>{ min_tokens_out_str.clone() }</td> }

                if show(22) { <td class={classes!("dim-col", (hc == Some(col_positions[22])).then_some("col-hover"))} style={border_style(22)} onmouseenter={make_hover_cb(col_positions[22])}>{ cu_limit_str.clone() }</td> }

                if show(23) {
                    <td class={(hc == Some(col_positions[23])).then_some("col-hover")} style={border_style(23)} onmouseenter={make_hover_cb(col_positions[23])}>
                        { &cu_price }
                    </td>
                }

                if show(24) {
                    <td class={classes!("dim-col", (hc == Some(col_positions[24])).then_some("col-hover"))} style={border_style(24)} onmouseenter={make_hover_cb(col_positions[24])}>{ s.ix_labels_count.to_string() }</td>
                }

                if show(25) {
                    <td class={classes!("labels-col", (hc == Some(col_positions[25])).then_some("col-hover"))} style={border_style(25)} onmouseenter={make_hover_cb(col_positions[25])} title={ix_labels_str.clone()}>{ ix_labels_str.clone() }</td>
                }

                if show(26) {
                    <td class={(hc == Some(col_positions[26])).then_some("col-hover")} style={border_style(26)} onmouseenter={make_hover_cb(col_positions[26])}>
                        { if s.is_migrated {
                            html! { <span class="status-badge status-true">{ "True" }</span> }
                        } else {
                            html! {}
                        } }
                    </td>
                }

                if show(27) {
                    <td class={(hc == Some(col_positions[27])).then_some("col-hover")} style={border_style(27)} onmouseenter={make_hover_cb(col_positions[27])}>
                        { if s.is_mayhem_mode {
                            html! { <span class="status-badge status-true">{ "True" }</span> }
                        } else {
                            html! {}
                        } }
                    </td>
                }

                <td class={classes!("row-actions", (hc == Some(action_col_pos)).then_some("col-hover"))} onmouseenter={make_hover_cb(action_col_pos)}>
                    <button class="row-select-btn" onclick={onclick} title="View details">
                        <svg width="15" height="15" viewBox="0 0 24 24" fill="none"
                             xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
                            <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.4"/>
                            <path d="M16 9l-5 7-3-3" stroke="currentColor"
                                  stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
                        </svg>
                    </button>
                </td>
            </tr>

            { if props.selected {
                html! {
                    <tr
                        id={format!("detail-{}", s.mint_address)}
                        class={classes!("detail-row", "open")}
                    >
                        <td colspan={(2 + props.visible_cols.iter().filter(|&&b| b).count()).to_string()}>{ detail_panel }</td>
                    </tr>
                }
            } else {
                html! {}
            } }
        </>
    }
}

// ── Instruction snapshot helper ───────────────────────────────────────────────

fn build_instruction_html(instr: &Value) -> Html {
    let value_to_string = |v: &Value| -> String {
        if let Some(s) = v.as_str() {
            s.to_string()
        } else {
            v.to_string()
        }
    };

    let items: Vec<String> = if let Some(obj) = instr.as_object() {
        obj.get("instructions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(|v| value_to_string(v)).collect())
            .unwrap_or_default()
    } else if let Some(arr) = instr.as_array() {
        arr.iter().map(|v| value_to_string(v)).collect()
    } else {
        vec![]
    };

    if items.is_empty() {
        return html! {
            <pre class="instruction-snapshot">
                { serde_json::to_string_pretty(instr).unwrap_or_else(|_| "{}".into()) }
            </pre>
        };
    }

    html! {
        <div class="instruction-text-list">
            { for items.iter().map(|text| html! {
                <div class="instruction-text-item">{ text }</div>
            }) }
        </div>
    }
}

// ── TokenDetailPanel — standalone detail panel for use with DataTable ─────────

#[derive(Properties, PartialEq)]
pub struct TokenDetailPanelProps {
    pub detail: Option<TokenDetailRecord>,
    pub loading: bool,
    #[prop_or_default]
    pub error: Option<String>,
}

#[function_component(TokenDetailPanel)]
pub fn token_detail_panel(props: &TokenDetailPanelProps) -> Html {
    let copy_copied = use_state(|| false);
    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above TokenDetailPanel");

    if props.loading {
        return html! {
            <div class="detail-loading">
                <span style="color:var(--text-dim); font-size:12px;">{ "Loading details..." }</span>
            </div>
        };
    }
    if let Some(err) = &props.error {
        return html! { <p class="error" style="padding:12px;">{ err }</p> };
    }
    let Some(detail) = &props.detail else {
        return html! {
            <div class="token-detail-panel-empty">
                <span style="color:var(--text-dim); font-size:12px;">
                    { "Select a row to load detailed info." }
                </span>
            </div>
        };
    };

    let d_fep = detail
        .initial_buy_sol
        .and_then(|buy| detail.initial_supply_token.map(|s| (buy, s)))
        .and_then(|(buy, supply)| if supply > 0 { Some(buy / supply as f64) } else { None });
    let d_fep_str = d_fep.map(format_price).unwrap_or_else(|| "-".into());
    let d_ath_mult = d_fep.and_then(|fep| detail.ath_price.and_then(|ath| if fep != 0.0 { Some(ath / fep) } else { None }));
    let d_ath_pct = d_fep.and_then(|fep| detail.ath_price.and_then(|ath| if fep != 0.0 { Some((ath / fep) * 100.0) } else { None }));
    let d_ath_mult_str = d_ath_mult.map(|v| format!("{}x  ({}%)", format_decimal_trim(v, 2), format_decimal(d_ath_pct.unwrap_or(0.0), 1))).unwrap_or_else(|| "-".into());
    let d_cur_mult = d_fep.and_then(|fep| detail.current_price.and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None }));
    let d_cur_pct = d_fep.and_then(|fep| detail.current_price.and_then(|cur| if fep != 0.0 { Some((cur / fep) * 100.0) } else { None }));
    let d_cur_mult_str = d_cur_mult.map(|v| format!("{}x  ({}%)", format_decimal_trim(v, 2), format_decimal(d_cur_pct.unwrap_or(0.0), 1))).unwrap_or_else(|| "-".into());
    let d_ath_str = detail.ath_price.map(|v| price_unit.display_price(v)).unwrap_or_else(|| "-".into());
    let d_cur_str = detail.current_price.map(|v| price_unit.display_price(v)).unwrap_or_else(|| "-".into());
    let d_ath_ts = detail.ath_timestamp.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
    let d_last_trade = detail.last_trade_at.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
    let d_volume = detail.volume_sol_total.map(|v| price_unit.display_compact(v, 4)).unwrap_or_else(|| "-".into());
    let d_mcap = detail.market_cap.map(|v| price_unit.display_compact(v, 4)).unwrap_or_else(|| "-".into());
    let d_trades = detail.trade_count.map_or_else(|| "-".into(), |v| v.to_string());
    let d_wallets = detail.unique_wallets_in_window.map_or_else(|| "-".into(), |v| v.to_string());
    let d_init_buy = detail.initial_buy_sol.map(|v| price_unit.display_amount(v)).unwrap_or_else(|| "-".into());
    let d_init_supply = detail.initial_supply_token.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let d_cu_limit = detail.cu_limit.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let d_cu_price = detail.cu_price.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let d_label_count = detail.instruction_labels.as_array().map(|a| a.len()).unwrap_or(0);
    let d_created = format_iso(&detail.created_at);
    let d_status = if detail.is_migrated { "Migrated ✓" } else { "Bonding Curve" };
    let d_status_cls = if detail.is_migrated { "detail-status-migrated" } else { "detail-status-bonding" };
    let d_symbol = if detail.symbol.is_empty() { truncate(&detail.mint_address, 8) } else { detail.symbol.clone() };

    let creator_solscan = format!("https://solscan.io/account/{}", detail.creator_address);
    let creator_gmgn = format!("https://gmgn.ai/sol/address/{}", detail.creator_address);
    let mint_solscan = format!("https://solscan.io/token/{}", detail.mint_address);
    let mint_gmgn = format!("https://gmgn.ai/sol/token/{}", detail.mint_address);
    let create_tx_solscan = format!("https://solscan.io/tx/{}", detail.create_tx_address);
    let creator_short = truncate(&detail.creator_address, 12);
    let mint_short = truncate(&detail.mint_address, 12);
    let create_tx_short = truncate(&detail.create_tx_address, 12);
    let bonding_short = detail.bonding_curve_address.as_deref().map(|a| truncate(a, 12)).unwrap_or_else(|| "-".into());
    let bonding_full = detail.bonding_curve_address.clone().unwrap_or_default();
    let bonding_solscan = detail.bonding_curve_address.as_ref().map(|a| format!("https://solscan.io/account/{}", a));
    let bonding_gmgn = detail.bonding_curve_address.as_ref().map(|a| format!("https://gmgn.ai/sol/address/{}", a));
    let bonding_html = if let Some(url) = bonding_solscan {
        html! { <AddrCard label="Bonding Curve" short={bonding_short} full={bonding_full} solscan_url={url} gmgn_url={bonding_gmgn} /> }
    } else {
        html! { <StatCard label="Bonding Curve" value="-" variant={StatVariant::Muted} /> }
    };

    let on_copy_labels = {
        let instruction_labels = detail.instruction_labels.clone();
        let copied = copy_copied.clone();
        Callback::from(move |_: MouseEvent| {
            let text = serde_json::to_string(&instruction_labels).unwrap_or_default();
            let copied = copied.clone();
            spawn_local(async move {
                if let Some(win) = web_sys::window() {
                    let cb = win.navigator().clipboard();
                    let _ = wasm_bindgen_futures::JsFuture::from(cb.write_text(&text)).await;
                    copied.set(true);
                    let copied_reset = copied.clone();
                    gloo::timers::callback::Timeout::new(1500, move || copied_reset.set(false)).forget();
                }
            });
        })
    };

    let instruction_html = build_instruction_html(&detail.instruction_labels);

    html! {
        <section class="token-detail-panel">
            <div class="detail-panel-inner">
                <div class="detail-header">
                    <div class="detail-title-group">
                        <span class="detail-symbol">{ &d_symbol }</span>
                        <span class="detail-token-name">{ &detail.name }</span>
                    </div>
                    <span class={classes!("detail-status-badge", d_status_cls)}>{ d_status }</span>
                </div>
                <div class="detail-body">
                    <div class="detail-left">
                        <div class="detail-section">
                            <div class="detail-section-title">{ "Price Performance" }</div>
                            <div class="stat-grid-3">
                                <StatCard label="First Entry Price" value={d_fep_str} large=true />
                                <StatCard label="ATH Price" value={d_ath_str} variant={StatVariant::Primary} large=true />
                                <StatCard label="Current Price" value={d_cur_str} large=true />
                                <StatCard label="ATH / FEP" value={d_ath_mult_str} variant={ratio_variant(d_ath_mult)} large=true />
                                <StatCard label="Current / FEP" value={d_cur_mult_str} variant={ratio_variant(d_cur_mult)} large=true />
                                <StatCard label="ATH Timestamp" value={d_ath_ts} variant={StatVariant::Muted} />
                            </div>
                        </div>
                        <div class="detail-section">
                            <div class="detail-section-title">{ "Activity & Market" }</div>
                            <div class="stat-grid-3">
                                <StatCard label={format!("Volume ({})", price_unit.unit_label())} value={d_volume} variant={StatVariant::Info} bold={true} />
                                <StatCard label={format!("Market Cap ({})", price_unit.unit_label())} value={d_mcap} bold={true} />
                                <StatCard label="Trade Count" value={d_trades} bold={true} />
                                <StatCard label="Unique Wallets" value={d_wallets} variant={StatVariant::Info} bold={true} />
                                <StatCard label="Last Trade" value={d_last_trade} variant={StatVariant::Muted} />
                                <StatCard label="Created" value={d_created} variant={StatVariant::Muted} />
                            </div>
                        </div>
                        <div class="detail-section">
                            <div class="detail-section-title">{ "Creation Parameters" }</div>
                            <div class="stat-grid-4">
                                <StatCard label={format!("Initial Buy ({})", price_unit.unit_label())} value={d_init_buy} />
                                <StatCard label="Initial Supply" value={d_init_supply} />
                                <StatCard label="CU Limit" value={d_cu_limit} variant={StatVariant::Muted} bold={true} />
                                <StatCard label="CU Price" value={d_cu_price} variant={StatVariant::Muted} bold={true} />
                            </div>
                        </div>
                        <div class="detail-section">
                            <div class="detail-section-title">{ "Addresses" }</div>
                            <div class="stat-grid-4">
                                <AddrCard label="Creator" short={creator_short} full={detail.creator_address.clone()} solscan_url={creator_solscan} gmgn_url={Some(creator_gmgn)} />
                                <AddrCard label="Mint" short={mint_short} full={detail.mint_address.clone()} solscan_url={mint_solscan} gmgn_url={Some(mint_gmgn)} />
                                <AddrCard label="Create TX" short={create_tx_short} full={detail.create_tx_address.clone()} solscan_url={create_tx_solscan} />
                                { bonding_html }
                            </div>
                        </div>
                    </div>
                    <div class="detail-body-divider"></div>
                    <div class="detail-right">
                        <div class="detail-section-title">
                            <span>{ format!("Instruction Labels  ({})", d_label_count) }</span>
                            <button
                                class={classes!("detail-copy-btn", (*copy_copied).then_some("detail-copy-ok"))}
                                onclick={on_copy_labels}
                                title={if *copy_copied { "Copied!" } else { "Copy labels to clipboard" }}
                            >
                                { if *copy_copied {
                                    html! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 13l4 4L19 7" /></svg> }
                                } else {
                                    html! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" /></svg> }
                                } }
                            </button>
                        </div>
                        { instruction_html }
                    </div>
                </div>
            </div>
        </section>
    }
}
