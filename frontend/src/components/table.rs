use yew::prelude::*;

use crate::state::transactions::LiveTrade;
use crate::utils::date::format_iso;
use crate::utils::format::{format_decimal, truncate};

const PAGE_SIZE_OPTIONS: &[usize] = &[10, 25, 50, 100];

// ── RowCells ──────────────────────────────────────────────────────────────────

/// A pre-rendered table row. Each element is a complete `<td>…</td>` node.
///
/// `PartialEq` is always-false because `Html` nodes are not structurally
/// comparable; this ensures `AppTable` always re-renders when the parent
/// passes new data.
#[derive(Clone)]
pub struct RowCells {
    pub cells: Vec<Html>,
    pub class: Option<Classes>,
    pub style: Option<AttrValue>,
    pub onclick: Option<Callback<MouseEvent>>,
}

impl RowCells {
    pub fn new(cells: Vec<Html>) -> Self {
        Self {
            cells,
            class: None,
            style: None,
            onclick: None,
        }
    }
}

impl PartialEq for RowCells {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

// ── Row builder: live trades ──────────────────────────────────────────────────

/// Build a `RowCells` for one `LiveTrade`. Shared by Dashboard and Transactions.
pub fn trade_row(ev: &LiveTrade) -> RowCells {
    let is_buy = ev.trade_type == "buy";
    let side_class = if is_buy { "side-buy" } else { "side-sell" };
    let side_label = if is_buy { "BUY" } else { "SELL" };
    let num_class = if is_buy { "num-buy" } else { "num-sell" };

    RowCells::new(vec![
        html! {
            <td class="addr" title={ev.mint.clone()}>
                <a href={format!("https://solscan.io/token/{}", ev.mint)}
                   target="_blank" rel="noopener noreferrer">
                    { truncate(&ev.mint, 10) }
                </a>
            </td>
        },
        html! { <td><span class={side_class}>{ side_label }</span></td> },
        html! {
            <td class="addr" title={ev.wallet.clone()}>
                <a href={format!("https://solscan.io/account/{}", ev.wallet)}
                   target="_blank" rel="noopener noreferrer">
                    { truncate(&ev.wallet, 10) }
                </a>
            </td>
        },
        html! { <td class={num_class}>{ format_decimal(ev.sol_amount, 4) }</td> },
        html! { <td class={num_class}>{ format_decimal(ev.token_amount, 0) }</td> },
        html! { <td class={num_class}>{ format_decimal(ev.price_per_token, 9) }</td> },
        html! {
            <td class="addr" title={ev.tx_signature.clone()}>
                <a href={format!("https://solscan.io/tx/{}", ev.tx_signature)}
                   target="_blank" rel="noopener noreferrer">
                    { truncate(&ev.tx_signature, 10) }
                </a>
            </td>
        },
        html! { <td>{ ev.slot.to_string() }</td> },
        html! { <td>{ format_iso(&ev.timestamp) }</td> },
    ])
}

// ── AppTable component ────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct AppTableProps {
    /// Column header labels (the leading "#" row-number column is added automatically).
    pub headers: Vec<AttrValue>,
    /// All rows (pre-rendered). Pagination is handled internally unless `paginate=false`.
    pub rows: Vec<RowCells>,
    /// Short label shown in the controls bar, e.g. `"tokens"` or `"trades"`.
    #[prop_or(AttrValue::Static("rows"))]
    pub label: AttrValue,
    /// Message shown when `rows` is empty.
    #[prop_or(AttrValue::Static("No data."))]
    pub empty_message: AttrValue,
    /// When `false`, render all rows without internal pagination controls.
    /// The parent is responsible for passing the correct page slice and offset.
    #[prop_or(true)]
    pub paginate: bool,
    /// Added to row numbers. Use when the parent manages server-side pagination
    /// so rows on page 2+ are numbered correctly.
    #[prop_or_default]
    pub row_offset: usize,
}

#[function_component(AppTable)]
pub fn app_table(props: &AppTableProps) -> Html {
    let page = use_state(|| 1usize);
    let page_size = use_state(|| 25usize);

    let col_count = props.headers.len();
    let total = props.rows.len();

    // If paginate=false, show all rows (parent owns pagination).
    let (start, end, cur_page, total_pages) = if props.paginate {
        let tp = if total == 0 {
            1
        } else {
            (total + *page_size - 1) / *page_size
        };
        let cp = (*page).clamp(1, tp);
        let s = (cp - 1) * *page_size;
        let e = (s + *page_size).min(total);
        (s, e, cp, tp)
    } else {
        (0, total, 1, 1)
    };

    let on_prev = {
        let page = page.clone();
        let p = cur_page;
        Callback::from(move |_: MouseEvent| {
            if p > 1 {
                page.set(p - 1);
            }
        })
    };
    let on_next = {
        let page = page.clone();
        let p = cur_page;
        let tp = total_pages;
        Callback::from(move |_: MouseEvent| {
            if p < tp {
                page.set(p + 1);
            }
        })
    };
    let on_size_change = {
        let page_size = page_size.clone();
        let page = page.clone();
        Callback::from(move |e: Event| {
            let el: web_sys::HtmlSelectElement = e.target_unchecked_into();
            if let Ok(v) = el.value().parse::<usize>() {
                page_size.set(v);
                page.set(1);
            }
        })
    };

    html! {
        <div class="table-wrapper">
            if props.paginate {
                <div class="table-controls">
                    <span class="table-total">
                        { format!("{} {} — page {} / {}", total, props.label, cur_page, total_pages) }
                    </span>
                    <label class="page-size-label">
                        { "Per page: " }
                        <select onchange={on_size_change}>
                            { for PAGE_SIZE_OPTIONS.iter().map(|&s| html! {
                                <option value={s.to_string()} selected={s == *page_size}>
                                    { s.to_string() }
                                </option>
                            }) }
                        </select>
                    </label>
                </div>
            }

            <div class="table-scroll">
                <table class="trade-table">
                    <thead>
                        <tr>
                            <th>{ "#" }</th>
                            { for props.headers.iter().map(|h| html! { <th>{ h.clone() }</th> }) }
                        </tr>
                    </thead>
                    <tbody>
                        if props.rows.is_empty() {
                            <tr>
                                <td colspan={(col_count + 1).to_string()} class="no-data">
                                    { props.empty_message.clone() }
                                </td>
                            </tr>
                        } else {
                            { for props.rows[start..end].iter().enumerate().map(|(i, row)| {
                                let row_num = props.row_offset + start + i + 1;
                                html! {
                                    <tr class={row.class.clone()} style={row.style.clone()} onclick={row.onclick.clone()}>
                                        <td class="row-num">{ row_num }</td>
                                        { for row.cells.iter().cloned() }
                                    </tr>
                                }
                            }) }
                        }
                    </tbody>
                </table>
            </div>

            if props.paginate {
                <div class="pagination">
                    <button onclick={on_prev} disabled={cur_page <= 1}>{ "‹ Prev" }</button>
                    <span>{ format!("Page {cur_page} / {total_pages}") }</span>
                    <button onclick={on_next} disabled={cur_page >= total_pages}>{ "Next ›" }</button>
                </div>
            }
        </div>
    }
}
