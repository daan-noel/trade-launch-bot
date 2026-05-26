use yew::prelude::*;

use crate::components::stat_card::{AddrCard, StatCard, StatVariant};
use crate::services::api::{TokenDetailRecord, TokenRecord};
use serde_json::Value;
use crate::utils::date::format_iso;
use crate::utils::format::{
    age_class, format_age, format_compact, format_decimal, format_decimal_trim, format_price,
    format_with_commas, truncate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn price_class(price: Option<f64>) -> &'static str {
    match price {
        Some(v) if v != 0.0 => {
            let abs = v.abs();
            if abs >= 1.0        { "price-normal"   }
            else if abs >= 1e-3  { "price-e-3"      }
            else if abs >= 1e-6  { "price-e-6"      }
            else if abs >= 1e-9  { "price-e-9"      }
            else if abs >= 1e-12 { "price-e-12"     }
            else if abs >= 1e-15 { "price-e-15"     }
            else                 { "price-e-smaller"}
        }
        _ => "price-normal",
    }
}

/// CSS class for ratio cells -- `mult` is the price multiple (e.g. 10 = 10x ATH vs entry).
fn ratio_class(mult: Option<f64>) -> &'static str {
    match mult {
        Some(v) if v >= 100.0 => "ratio-moon",
        Some(v) if v >= 30.0  => "ratio-high",
        Some(v) if v >= 10.0  => "ratio-good",
        Some(v) if v >= 3.0   => "ratio-mid",
        Some(v) if v >= 1.5   => "ratio-low",
        _                     => "ratio-flat",
    }
}

/// Maps a price multiple to a `StatVariant` for detail-panel cards.
fn ratio_variant(mult: Option<f64>) -> StatVariant {
    match mult {
        Some(v) if v >= 100.0 => StatVariant::Danger,
        Some(v) if v >= 30.0  => StatVariant::Accent,
        Some(v) if v >= 10.0  => StatVariant::Warning,
        Some(v) if v >= 3.0   => StatVariant::Primary,
        Some(v) if v >= 1.5   => StatVariant::Info,
        _                     => StatVariant::Muted,
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
    /// Visibility mask — one bool per COLUMNS entry (symbol=0 … created=12).
    /// Defaults to all-visible when not supplied.
    #[prop_or_default]
    pub visible_cols: Vec<bool>,
}

// ── Update-flash tracking ─────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
struct UpdateFlags {
    symbol: bool,
    current_price: bool,
    ath_price: bool,
    ath_timestamp: bool,
    volume_sol_total: bool,
    market_cap: bool,
    initial_buy_sol: bool,
    initial_supply_token: bool,
    cu_limit: bool,
    cu_price: bool,
    ix_labels_count: bool,
    is_migrated: bool,
    age: bool,
    created_at: bool,
    ath_fep_ratio: bool,
    current_fep_ratio: bool,
    trade_count: bool,
    mayhem_mode: bool,
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

    let current_price_value = s.current_price;
    let current_price = current_price_value
        .map(format_price)
        .unwrap_or_else(|| "-".into());

    let first_entry_price_value = s
        .initial_buy_sol
        .and_then(|buy| s.initial_supply_token.map(|supply| (buy, supply)))
        .and_then(|(buy, supply)| if supply > 0 { Some(buy / supply as f64) } else { None });

    // ath_fep_ratio_value: (ath / fep) * 100  ->  price multiple x 100
    let ath_fep_ratio_value = first_entry_price_value.and_then(|fep| {
        s.ath_price.and_then(|ath| {
            if fep != 0.0 { Some((ath / fep) * 100.0) } else { None }
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
        current_price_value.and_then(|cur| {
            if fep != 0.0 { Some(cur / fep) } else { None }
        })
    });
    let cur_fep_display = current_fep_ratio_value
        .map(|v| format!("{}x", format_decimal_trim(v, 2)))
        .unwrap_or_else(|| "-".into());

    let ath_price_str = s.ath_price.map(format_price).unwrap_or_else(|| "-".into());
    let market_cap = s.market_cap.map(|v| format_compact(v, 3)).unwrap_or_else(|| "-".into());
    let initial_buy = s.initial_buy_sol.map(|v| format_decimal(v, 4)).unwrap_or_else(|| "-".into());
    let cu_price = s.cu_price.map(|v| format_with_commas(v)).unwrap_or_else(|| "-".to_string());
    let mayhem_mode_str: String = if s.is_mayhem_mode {
        "Yes".to_string()
    } else {
        "-".to_string()
    };

    let age_text  = format_age(s.age);
    let age_cls   = age_class(s.age);

    // ── Click handler ─────────────────────────────────────────────────────────
    let onclick = {
        let on_select = props.on_select.clone();
        let mint = s.mint_address.clone();
        Callback::from(move |_: MouseEvent| on_select.emit(mint.clone()))
    };

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
                .and_then(|(buy, supply)| if supply > 0 { Some(buy / supply as f64) } else { None });
            let d_fep_str = d_fep.map(format_price).unwrap_or_else(|| "-".into());

            let d_ath_mult = d_fep.and_then(|fep| {
                detail.ath_price.and_then(|ath| if fep != 0.0 { Some(ath / fep) } else { None })
            });
            let d_ath_pct = d_fep.and_then(|fep| {
                detail.ath_price.and_then(|ath| if fep != 0.0 { Some((ath / fep) * 100.0) } else { None })
            });
            let d_ath_mult_str = d_ath_mult
                .map(|v| format!("{}x  ({}%)", format_decimal_trim(v, 2), format_decimal(d_ath_pct.unwrap_or(0.0), 1)))
                .unwrap_or_else(|| "-".into());

            let d_cur_mult = d_fep.and_then(|fep| {
                detail.current_price.and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
            });
            let d_cur_pct = d_fep.and_then(|fep| {
                detail.current_price.and_then(|cur| if fep != 0.0 { Some((cur / fep) * 100.0) } else { None })
            });
            let d_cur_mult_str = d_cur_mult
                .map(|v| format!("{}x  ({}%)", format_decimal_trim(v, 2), format_decimal(d_cur_pct.unwrap_or(0.0), 1)))
                .unwrap_or_else(|| "-".into());

            let d_ath_str = detail.ath_price.map(format_price).unwrap_or_else(|| "-".into());
            let d_cur_str = detail.current_price.map(format_price).unwrap_or_else(|| "-".into());
            let d_ath_ts = detail.ath_timestamp.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
            let d_last_trade = detail.last_trade_at.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
            let d_volume = detail.volume_sol_total.map(|v| format_compact(v, 4)).unwrap_or_else(|| "-".into());
            let d_mcap = detail.market_cap.map(|v| format_compact(v, 4)).unwrap_or_else(|| "-".into());
            let d_trades = detail.trade_count.map_or_else(|| "-".into(), |v| v.to_string());
            let d_wallets = detail.unique_wallets_in_window.map_or_else(|| "-".into(), |v| v.to_string());
            let d_init_buy = detail.initial_buy_sol.map(|v| format_decimal(v, 4)).unwrap_or_else(|| "-".into());
            let d_init_supply = detail.initial_supply_token.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
            let d_cu_limit = detail.cu_limit.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
            let d_cu_price = detail.cu_price.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
            let d_label_count = detail.instruction_labels.as_array().map(|a| a.len()).unwrap_or(0);
            let d_created = format_iso(&detail.created_at);
            let d_status = if detail.is_migrated { "Migrated ✓" } else { "Bonding Curve" };
            let d_status_cls = if detail.is_migrated { "detail-status-migrated" } else { "detail-status-bonding" };

            let d_symbol = if detail.symbol.is_empty() {
                truncate(&detail.mint_address, 8)
            } else {
                detail.symbol.clone()
            };

            let creator_solscan   = format!("https://solscan.io/account/{}", detail.creator_address);
            let creator_gmgn      = format!("https://gmgn.ai/sol/address/{}", detail.creator_address);
            let mint_solscan      = format!("https://solscan.io/token/{}", detail.mint_address);
            let mint_gmgn         = format!("https://gmgn.ai/sol/token/{}", detail.mint_address);
            let create_tx_solscan = format!("https://solscan.io/tx/{}", detail.create_tx_address);

            let creator_short   = truncate(&detail.creator_address, 12);
            let mint_short      = truncate(&detail.mint_address, 12);
            let create_tx_short = truncate(&detail.create_tx_address, 12);
            let bonding_short   = detail.bonding_curve_address.as_deref()
                .map(|a| truncate(a, 12))
                .unwrap_or_else(|| "-".into());
            let bonding_full    = detail.bonding_curve_address.clone().unwrap_or_default();
            let bonding_solscan = detail.bonding_curve_address.as_ref()
                .map(|a| format!("https://solscan.io/account/{}", a));
            let bonding_gmgn    = detail.bonding_curve_address.as_ref()
                .map(|a| format!("https://gmgn.ai/sol/address/{}", a));
            let bonding_html = if let Some(url) = bonding_solscan {
                html! { <AddrCard label="Bonding Curve" short={bonding_short} full={bonding_full} solscan_url={url} gmgn_url={bonding_gmgn} /> }
            } else {
                html! { <StatCard label="Bonding Curve" value="-" variant={StatVariant::Muted} /> }
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
                                <StatCard label="Volume (SOL)" value={d_volume} variant={StatVariant::Info} bold={true} />
                                <StatCard label="Market Cap (SOL)" value={d_mcap} bold={true} />
                                <StatCard label="Trade Count" value={d_trades} bold={true} />
                                <StatCard label="Unique Wallets" value={d_wallets} variant={StatVariant::Info} bold={true} />
                                <StatCard label="Last Trade" value={d_last_trade} variant={StatVariant::Muted} />
                                <StatCard label="Created" value={d_created} variant={StatVariant::Muted} />
                            </div>
                        </div>

                        <div class="detail-section">
                            <div class="detail-section-title">{ "Creation Parameters" }</div>
                            <div class="stat-grid-4">
                                <StatCard label="Initial Buy (SOL)" value={d_init_buy} />
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
                                { format!("Instruction Labels  ({})", d_label_count) }
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
                flags.symbol               = prev.symbol != token.symbol;
                flags.current_price        = prev.current_price != token.current_price;
                flags.ath_price            = prev.ath_price != token.ath_price;
                flags.ath_timestamp        = prev.ath_timestamp != token.ath_timestamp;
                flags.volume_sol_total     = prev.volume_sol_total != token.volume_sol_total;
                flags.market_cap           = prev.market_cap != token.market_cap;
                flags.initial_buy_sol      = prev.initial_buy_sol != token.initial_buy_sol;
                flags.initial_supply_token = prev.initial_supply_token != token.initial_supply_token;
                flags.cu_limit             = prev.cu_limit != token.cu_limit;
                flags.cu_price             = prev.cu_price != token.cu_price;
                flags.ix_labels_count      = prev.ix_labels_count != token.ix_labels_count;
                flags.is_migrated          = prev.is_migrated != token.is_migrated;
                flags.mayhem_mode          = prev.is_mayhem_mode != token.is_mayhem_mode;
                flags.age                  = prev.age != token.age;
                flags.created_at           = prev.created_at != token.created_at;
                flags.trade_count          = prev.trade_count != token.trade_count;

                let prev_fep = prev.initial_buy_sol
                    .and_then(|buy| prev.initial_supply_token.map(|s| (buy, s)))
                    .and_then(|(buy, s)| if s > 0 { Some(buy / s as f64) } else { None });
                let new_fep = token.initial_buy_sol
                    .and_then(|buy| token.initial_supply_token.map(|s| (buy, s)))
                    .and_then(|(buy, s)| if s > 0 { Some(buy / s as f64) } else { None });

                let prev_ath_fep = prev_fep.and_then(|fep| prev.ath_price.and_then(|ath| if fep != 0.0 { Some((ath / fep) * 100.0) } else { None }));
                let new_ath_fep  = new_fep .and_then(|fep| token.ath_price.and_then(|ath| if fep != 0.0 { Some((ath / fep) * 100.0) } else { None }));
                flags.ath_fep_ratio = prev_ath_fep != new_ath_fep;

                let prev_cur_fep = prev_fep.and_then(|fep| prev.current_price.and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None }));
                let new_cur_fep  = new_fep .and_then(|fep| token.current_price.and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None }));
                flags.current_fep_ratio = prev_cur_fep != new_cur_fep;
            }
            *previous.borrow_mut() = Some(token.clone());
            update_flags.set(flags);
            || ()
        });
    }

    let row_class = if props.selected { classes!("selected-row") } else { classes!() };
    let row_num_str = props.row_num.map(|n| n.to_string()).unwrap_or_default();
    // Helper: returns true when column i should be rendered (default-show when vec not supplied)
    let show = |i: usize| props.visible_cols.get(i).copied().unwrap_or(true);

    // ── Extra column display values (indices 13–21) ───────────────────────────
    let cu_limit_str    = s.cu_limit.map(|v| format_with_commas(v)).unwrap_or_else(|| "-".into());
    let init_supply_str = s.initial_supply_token.map(|v| format_with_commas(v)).unwrap_or_else(|| "-".into());
    let ath_ts_str      = s.ath_timestamp.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
    let last_trade_str  = s.last_trade_at.as_deref().map(format_iso).unwrap_or_else(|| "-".into());
    let ix_labels_str: String = s.instruction_labels.as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".into());

    html! {
        <>
            <tr id={format!("row-{}", s.mint_address)} class={row_class}>

                <td class="row-num">{ row_num_str }</td>

                // 0 — Symbol (Identity)
                if show(0) {
                    <td class={classes!(update_flags.symbol.then_some("updated-cell"))}>
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

                // 1 — Name (Identity)
                if show(1) { <td>{ s.name.clone() }</td> }

                // 2 — Mint (Identity)
                if show(2) {
                    <td class="addr" title={s.mint_address.clone()}>{ truncate(&s.mint_address, 12) }</td>
                }

                // 3 — Creator (Identity)
                if show(3) {
                    <td class="addr" title={s.creator_address.clone()}>{ truncate(&s.creator_address, 12) }</td>
                }

                // 4 — Age (Lifecycle)
                if show(4) {
                    <td class={classes!(age_cls, update_flags.age.then_some("updated-cell"))}>
                        { age_text }
                    </td>
                }

                // 5 — Created (Lifecycle)
                if show(5) {
                    <td class={classes!(update_flags.created_at.then_some("updated-cell"))}>
                        { format_iso(&s.created_at) }
                    </td>
                }

                // 6 — Last Trade (Lifecycle)
                if show(6) { <td class="dim-col">{ last_trade_str.clone() }</td> }

                // 7 — Migrated (Lifecycle)
                if show(7) {
                    <td class={classes!(update_flags.is_migrated.then_some("updated-cell"))}>
                        { if s.is_migrated {
                            html! { <span class="migrated-yes">{ "V" }</span> }
                        } else {
                            html! { <span class="migrated-no">{ "-" }</span> }
                        } }
                    </td>
                }

                // 8 — Mayhem Mode (Lifecycle)
                if show(8) {
                    <td class={classes!(update_flags.mayhem_mode.then_some("updated-cell"))}>
                        { &mayhem_mode_str }
                    </td>
                }

                // 9 — ATH/FEP (Performance)
                if show(9) {
                    <td class={classes!(
                        "ath-fep-col",
                        ratio_class(ath_fep_mult),
                        update_flags.ath_fep_ratio.then_some("updated-cell")
                    )}>
                        <div class="ratio-cell">
                            <span class="ratio-main">{ &ath_fep_display }</span>
                            <span class="ratio-sub">{ &ath_fep_pct }</span>
                        </div>
                    </td>
                }

                // 9 — Cur/FEP (Performance)
                if show(9) {
                    <td class={classes!(
                        "cur-fep-col",
                        ratio_class(current_fep_ratio_value),
                        update_flags.current_fep_ratio.then_some("updated-cell")
                    )}>
                        { &cur_fep_display }
                    </td>
                }

                // 10 — ATH price (Performance)
                if show(10) {
                    <td class={classes!(price_class(s.ath_price), update_flags.ath_price.then_some("updated-cell"))}>
                        { &ath_price_str }
                    </td>
                }

                // 11 — ATH At (Performance)
                if show(11) { <td class="dim-col">{ ath_ts_str.clone() }</td> }

                // 12 — Current Price (Performance)
                if show(12) {
                    <td class={classes!(price_class(current_price_value), update_flags.current_price.then_some("updated-cell"))}>
                        { &current_price }
                    </td>
                }

                // 13 — MCap (Market)
                if show(13) {
                    <td class={classes!(price_class(s.market_cap), update_flags.market_cap.then_some("updated-cell"))}>
                        { &market_cap }
                    </td>
                }

                // 14 — Volume (Market)
                if show(14) {
                    <td class={classes!(update_flags.volume_sol_total.then_some("updated-cell"))}>
                        { format_compact(s.volume_sol_total, 2) }
                    </td>
                }

                // 15 — Init Buy (Market)
                if show(15) {
                    <td class={classes!(update_flags.initial_buy_sol.then_some("updated-cell"))}>
                        { &initial_buy }
                    </td>
                }

                // 16 — Init Supply (Market)
                if show(16) { <td class="dim-col">{ init_supply_str.clone() }</td> }

                // 17 — Trades (Market)
                if show(17) {
                    <td class={classes!(update_flags.trade_count.then_some("updated-cell"))}>
                        { format_with_commas(s.trade_count) }
                    </td>
                }

                // 18 — CU Limit (Technical)
                if show(18) { <td class="dim-col">{ cu_limit_str.clone() }</td> }

                // 19 — CU Price (Technical)
                if show(19) {
                    <td class={classes!(update_flags.cu_price.then_some("updated-cell"))}>
                        { &cu_price }
                    </td>
                }

                // 20 — IX Count (Technical)
                if show(20) {
                    <td class="dim-col">{ s.ix_labels_count.to_string() }</td>
                }

                // 21 — IX Labels (Technical)
                if show(21) {
                    <td class="labels-col" title={ix_labels_str.clone()}>{ ix_labels_str.clone() }</td>
                }

                // 22 — Create TX (Technical)
                if show(22) {
                    <td class="addr" title={s.create_tx_address.clone()}>{ truncate(&s.create_tx_address, 12) }</td>
                }

                <td class="row-actions">
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
        if let Some(s) = v.as_str() { s.to_string() }
        else { v.to_string() }
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
