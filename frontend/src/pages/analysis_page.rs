use gloo::timers::callback::Interval;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::components::{AppTable, Header, RowCells};
use crate::services::api::{
    fetch_analysis, fetch_creators, AnalysisRecord, CreatorRecord, POLL_INTERVAL_MS,
};
use crate::utils::date::format_iso;
use crate::utils::format::{format_decimal, truncate};

// ── Score badge ───────────────────────────────────────────────────────────────

fn score_badge(score: f64) -> Html {
    let cls = if score >= 0.7 {
        "score-high"
    } else if score >= 0.4 {
        "score-mid"
    } else {
        "score-low"
    };
    html! { <span class={cls}>{ format!("{:.0}%", score * 100.0) }</span> }
}

// ── Tab enum ─────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum Tab {
    Creators,
    Results,
}

// ── Row builders ─────────────────────────────────────────────────────────────

const CREATOR_HEADERS: &[&str] = &[
    "Wallet",
    "Tokens Created",
    "Volume (SOL)",
    "Suspiciousness",
    "Wash Trade",
    "Last Analyzed",
];

fn creator_row(c: &CreatorRecord) -> RowCells {
    let wallet_url = format!("https://solscan.io/account/{}", c.wallet_address);
    let last_analyzed = c
        .last_analyzed_at
        .as_deref()
        .map(format_iso)
        .unwrap_or_else(|| "-".into());

    RowCells::new(vec![
        html! {
            <td class="addr" title={c.wallet_address.clone()}>
                <a href={wallet_url} target="_blank" rel="noopener noreferrer">
                    { truncate(&c.wallet_address, 12) }
                </a>
            </td>
        },
        html! { <td>{ c.tokens_created.to_string() }</td> },
        html! { <td>{ format_decimal(c.total_volume_sol, 4) }</td> },
        html! { <td>{ score_badge(c.suspiciousness_score) }</td> },
        html! { <td>{ score_badge(c.wash_trade_score) }</td> },
        html! { <td>{ last_analyzed }</td> },
    ])
}

const RESULT_HEADERS: &[&str] = &["Analyzer", "Score", "Indicators", "Computed At"];

fn analysis_row(r: &AnalysisRecord) -> RowCells {
    let indicators = if r.indicators.is_empty() {
        "-".to_string()
    } else {
        r.indicators.join(" · ")
    };

    RowCells::new(vec![
        html! { <td>{ r.analyzer_name.clone() }</td> },
        html! { <td>{ score_badge(r.score) }</td> },
        html! { <td class="indicator-cell" title={indicators.clone()}>{ indicators }</td> },
        html! { <td>{ format_iso(&r.computed_at) }</td> },
    ])
}

// ── Page ──────────────────────────────────────────────────────────────────────

const PAGE_SIZE_OPTIONS: &[usize] = &[10, 25, 50, 100];

#[function_component(AnalysisPage)]
pub fn analysis_page() -> Html {
    let tab = use_state(|| Tab::Creators);

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

    // ── Creator state ─────────────────────────────────────────────────────────
    let creators = use_state(Vec::<CreatorRecord>::new);
    let creator_total = use_state(|| 0i64);
    let creator_page = use_state(|| 1usize);
    let creator_ps = use_state(|| 25usize);
    let creator_load = use_state(|| true);
    let creator_err = use_state(|| Option::<String>::None);

    // ── Analysis result state ─────────────────────────────────────────────────
    let results = use_state(Vec::<AnalysisRecord>::new);
    let result_total = use_state(|| 0i64);
    let result_page = use_state(|| 1usize);
    let result_ps = use_state(|| 25usize);
    let result_load = use_state(|| true);
    let result_err = use_state(|| Option::<String>::None);

    // ── Fetch creators ────────────────────────────────────────────────────────
    {
        let creators = creators.clone();
        let creator_total = creator_total.clone();
        let creator_load = creator_load.clone();
        let creator_err = creator_err.clone();
        let page_val = *creator_page;
        let ps_val = *creator_ps;
        let tick_val = *tick;
        use_effect_with((page_val, ps_val, tick_val), move |_| {
            let offset = page_val.saturating_sub(1) * ps_val;
            spawn_local(async move {
                match fetch_creators(ps_val as i64, offset as i64).await {
                    Ok(r) => {
                        creator_total.set(r.total);
                        creators.set(r.items);
                        creator_err.set(None);
                    }
                    Err(e) => creator_err.set(Some(e)),
                }
                creator_load.set(false);
            });
            || ()
        });
    }

    // ── Fetch analysis results ────────────────────────────────────────────────
    {
        let results = results.clone();
        let result_total = result_total.clone();
        let result_load = result_load.clone();
        let result_err = result_err.clone();
        let page_val = *result_page;
        let ps_val = *result_ps;
        let tick_val = *tick;
        use_effect_with((page_val, ps_val, tick_val), move |_| {
            let offset = page_val.saturating_sub(1) * ps_val;
            spawn_local(async move {
                match fetch_analysis(ps_val as i64, offset as i64).await {
                    Ok(r) => {
                        result_total.set(r.total);
                        results.set(r.items);
                        result_err.set(None);
                    }
                    Err(e) => result_err.set(Some(e)),
                }
                result_load.set(false);
            });
            || ()
        });
    }

    // ── Creator pagination ────────────────────────────────────────────────────
    let c_total = *creator_total as usize;
    let c_ps = *creator_ps;
    let c_total_pg = if c_total == 0 {
        1
    } else {
        (c_total + c_ps - 1) / c_ps
    };
    let c_cur = (*creator_page).clamp(1, c_total_pg);
    let c_offset = c_cur.saturating_sub(1) * c_ps;

    let on_c_prev = {
        let p = creator_page.clone();
        let cp = c_cur;
        Callback::from(move |_: MouseEvent| {
            if cp > 1 {
                p.set(cp - 1);
            }
        })
    };
    let on_c_next = {
        let p = creator_page.clone();
        let cp = c_cur;
        let tp = c_total_pg;
        Callback::from(move |_: MouseEvent| {
            if cp < tp {
                p.set(cp + 1);
            }
        })
    };
    let on_c_ps = {
        let ps = creator_ps.clone();
        let p = creator_page.clone();
        Callback::from(move |e: Event| {
            let el: web_sys::HtmlSelectElement =
                wasm_bindgen::JsCast::dyn_into(e.target().unwrap()).unwrap();
            if let Ok(v) = el.value().parse::<usize>() {
                ps.set(v);
                p.set(1);
            }
        })
    };

    // ── Result pagination ─────────────────────────────────────────────────────
    let r_total = *result_total as usize;
    let r_ps = *result_ps;
    let r_total_pg = if r_total == 0 {
        1
    } else {
        (r_total + r_ps - 1) / r_ps
    };
    let r_cur = (*result_page).clamp(1, r_total_pg);
    let r_offset = r_cur.saturating_sub(1) * r_ps;

    let on_r_prev = {
        let p = result_page.clone();
        let cp = r_cur;
        Callback::from(move |_: MouseEvent| {
            if cp > 1 {
                p.set(cp - 1);
            }
        })
    };
    let on_r_next = {
        let p = result_page.clone();
        let cp = r_cur;
        let tp = r_total_pg;
        Callback::from(move |_: MouseEvent| {
            if cp < tp {
                p.set(cp + 1);
            }
        })
    };
    let on_r_ps = {
        let ps = result_ps.clone();
        let p = result_page.clone();
        Callback::from(move |e: Event| {
            let el: web_sys::HtmlSelectElement =
                wasm_bindgen::JsCast::dyn_into(e.target().unwrap()).unwrap();
            if let Ok(v) = el.value().parse::<usize>() {
                ps.set(v);
                p.set(1);
            }
        })
    };

    // ── Tab callbacks ─────────────────────────────────────────────────────────
    let set_creators = {
        let t = tab.clone();
        Callback::from(move |_: MouseEvent| t.set(Tab::Creators))
    };
    let set_results = {
        let t = tab.clone();
        Callback::from(move |_: MouseEvent| t.set(Tab::Results))
    };

    let c_rows = (*creators).iter().map(creator_row).collect::<Vec<_>>();
    let c_hdrs = CREATOR_HEADERS
        .iter()
        .map(|&h| AttrValue::Static(h))
        .collect::<Vec<_>>();

    let r_rows = (*results).iter().map(analysis_row).collect::<Vec<_>>();
    let r_hdrs = RESULT_HEADERS
        .iter()
        .map(|&h| AttrValue::Static(h))
        .collect::<Vec<_>>();

    html! {
        <div class="page-shell">
            <Header />
            <main class="page-body">
                <div class="section-header">
                    <h2 class="section-title">{ "Analysis" }</h2>
                    <span class="token-count-badge">
                        { format!("{} creators · {} results", c_total, r_total) }
                    </span>
                </div>

                // ── Tab bar ───────────────────────────────────────────────────
                <div class="tab-bar">
                    <button
                        class={if *tab == Tab::Creators { "tab-btn active" } else { "tab-btn" }}
                        onclick={set_creators}
                    >
                        { "Creator Profiles" }
                    </button>
                    <button
                        class={if *tab == Tab::Results { "tab-btn active" } else { "tab-btn" }}
                        onclick={set_results}
                    >
                        { "Analysis Results" }
                    </button>
                </div>

                // ── Creator Profiles tab ──────────────────────────────────────
                if *tab == Tab::Creators {
                    <div class="tokens-filter-bar">
                        <label class="page-size-label">
                            { "Per page: " }
                            <select onchange={on_c_ps}>
                                { for PAGE_SIZE_OPTIONS.iter().map(|&s| html! {
                                    <option value={s.to_string()} selected={s == c_ps}>{ s.to_string() }</option>
                                }) }
                            </select>
                        </label>
                    </div>

                    if *creator_load {
                        <p class="loading">{ "Loading creator profiles…" }</p>
                    } else if let Some(err) = (*creator_err).clone() {
                        <p class="error">{ err }</p>
                    } else {
                        <AppTable
                            paginate={false}
                            row_offset={c_offset}
                            headers={c_hdrs}
                            rows={c_rows}
                            label="creators"
                            empty_message="No creator profiles yet."
                        />
                        <div class="pagination">
                            <button onclick={on_c_prev} disabled={c_cur <= 1}>{ "‹ Prev" }</button>
                            <span>{ format!("Page {} / {}  ({} total)", c_cur, c_total_pg, c_total) }</span>
                            <button onclick={on_c_next} disabled={c_cur >= c_total_pg}>{ "Next ›" }</button>
                        </div>
                    }
                }

                // ── Analysis Results tab ──────────────────────────────────────
                if *tab == Tab::Results {
                    <div class="tokens-filter-bar">
                        <label class="page-size-label">
                            { "Per page: " }
                            <select onchange={on_r_ps}>
                                { for PAGE_SIZE_OPTIONS.iter().map(|&s| html! {
                                    <option value={s.to_string()} selected={s == r_ps}>{ s.to_string() }</option>
                                }) }
                            </select>
                        </label>
                    </div>

                    if *result_load {
                        <p class="loading">{ "Loading analysis results…" }</p>
                    } else if let Some(err) = (*result_err).clone() {
                        <p class="error">{ err }</p>
                    } else {
                        <AppTable
                            paginate={false}
                            row_offset={r_offset}
                            headers={r_hdrs}
                            rows={r_rows}
                            label="results"
                            empty_message="No analysis results yet."
                        />
                        <div class="pagination">
                            <button onclick={on_r_prev} disabled={r_cur <= 1}>{ "‹ Prev" }</button>
                            <span>{ format!("Page {} / {}  ({} total)", r_cur, r_total_pg, r_total) }</span>
                            <button onclick={on_r_next} disabled={r_cur >= r_total_pg}>{ "Next ›" }</button>
                        </div>
                    }
                }
            </main>
        </div>
    }
}
