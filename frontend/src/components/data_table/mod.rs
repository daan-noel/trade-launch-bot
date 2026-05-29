pub mod column;
pub use column::{Column, SortDir, SortKey};

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use web_sys::HtmlInputElement;
use wasm_bindgen::JsValue;
use yew::prelude::*;

use crate::components::Pagination;

// ── Props ─────────────────────────────────────────────────────────────────────

#[derive(Properties)]
pub struct DataTableProps<R: PartialEq + Clone + 'static> {
    /// Column definitions (order determines display order).
    pub columns: Vec<Column<R>>,
    /// Full dataset. DataTable filters, sorts, and paginates internally.
    pub rows: Vec<R>,
    /// Extracts a unique string key from each row (used for selection tracking).
    pub row_key: Rc<dyn Fn(&R) -> String>,

    // ── Optional per-row features ─────────────────────────────────────────────
    /// Renders extra action buttons placed in a trailing "Actions" column.
    #[prop_or_default]
    pub row_actions: Option<Rc<dyn Fn(&R) -> Html>>,
    /// Renders an inline detail panel inserted after the selected row.
    #[prop_or_default]
    pub row_detail: Option<Rc<dyn Fn(&R) -> Html>>,

    // ── Selection ─────────────────────────────────────────────────────────────
    /// Called when the selected row changes; `None` means deselected.
    #[prop_or_default]
    pub on_select: Option<Callback<Option<String>>>,
    /// External selected key — when Some, overrides internal selection state.
    #[prop_or_default]
    pub selected_key: Option<String>,

    // ── Pagination ────────────────────────────────────────────────────────────
    #[prop_or(25)]
    pub default_page_size: usize,
    #[prop_or_default]
    pub page_size_options: Vec<usize>,

    // ── Feature flags ─────────────────────────────────────────────────────────
    #[prop_or(true)]
    pub searchable: bool,
    #[prop_or_default]
    pub col_filters: bool,
    #[prop_or_default]
    pub col_toggle: bool,
    #[prop_or_default]
    pub hoverable: bool,

    // ── Labels ────────────────────────────────────────────────────────────────
    #[prop_or(AttrValue::Static("items"))]
    pub item_label: AttrValue,
    #[prop_or(AttrValue::Static("No data."))]
    pub empty_message: AttrValue,

    // ── localStorage persistence for column visibility ────────────────────────
    #[prop_or_default]
    pub storage_key: Option<&'static str>,
}

impl<R: PartialEq + Clone + 'static> PartialEq for DataTableProps<R> {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
            && self.columns == other.columns
            && self.selected_key == other.selected_key
            && self.default_page_size == other.default_page_size
            && self.searchable == other.searchable
            && self.col_filters == other.col_filters
            && self.col_toggle == other.col_toggle
            && self.hoverable == other.hoverable
            && self.item_label == other.item_label
            && self.empty_message == other.empty_message
            && self.storage_key == other.storage_key
    }
}

// ── localStorage helpers ──────────────────────────────────────────────────────

fn ls_load_visible(storage_key: &str, columns: &[impl AsRef<str>]) -> HashSet<String> {
    let default: HashSet<String> = columns.iter().map(|c| c.as_ref().to_string()).collect();
    let Some(window) = web_sys::window() else { return default; };
    let Some(storage) = window.local_storage().ok().flatten() else { return default; };
    let Some(raw) = storage.get_item(storage_key).ok().flatten() else { return default; };
    match js_sys::JSON::parse(&raw) {
        Ok(obj) => {
            let arr = js_sys::Array::from(&obj);
            let parsed: HashSet<String> = arr
                .iter()
                .filter_map(|v| v.as_string())
                .filter(|s| columns.iter().any(|c| c.as_ref() == s.as_str()))
                .collect();
            if parsed.is_empty() { default } else { parsed }
        }
        Err(_) => default,
    }
}

fn ls_save_visible(storage_key: &str, cols: &HashSet<String>) {
    let Some(window) = web_sys::window() else { return; };
    let Some(storage) = window.local_storage().ok().flatten() else { return; };
    let arr = js_sys::Array::new();
    for k in cols { arr.push(&JsValue::from_str(k)); }
    if let Ok(json) = js_sys::JSON::stringify(&arr) {
        if let Some(s) = json.as_string() { let _ = storage.set_item(storage_key, &s); }
    }
}

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component]
pub fn DataTable<R: PartialEq + Clone + 'static>(props: &DataTableProps<R>) -> Html {
    // ── Internal state ────────────────────────────────────────────────────────
    let page = use_state(|| 1usize);
    let page_size = use_state(|| props.default_page_size);
    let sort_col = use_state(|| Option::<&'static str>::None);
    let sort_dir = use_state(SortDir::default);
    let search = use_state(String::new);
    let col_filter_map = use_state(HashMap::<String, String>::new);
    let visible_cols = {
        let cols = props.columns.iter().filter(|c| c.default_visible).map(|c| c.key.to_string()).collect::<Vec<_>>();
        let sk = props.storage_key;
        use_state(move || {
            if let Some(key) = sk {
                ls_load_visible(key, &cols)
            } else {
                cols.into_iter().collect()
            }
        })
    };
    let selected_key_internal = use_state(|| Option::<String>::None);
    let show_col_panel = use_state(|| false);
    let show_filter_row = use_state(|| false);
    let hovered_col = use_state(|| Option::<usize>::None);

    // ── Resolve selected key (external override or internal) ──────────────────
    let selected_key = props.selected_key.as_ref().or(selected_key_internal.as_ref());

    // ── Persist col visibility ────────────────────────────────────────────────
    {
        let vc = (*visible_cols).clone();
        let sk = props.storage_key;
        use_effect_with(vc, move |cols| {
            if let Some(key) = sk { ls_save_visible(key, cols); }
            || ()
        });
    }

    // ── Reset page when search/filter/sort changes ────────────────────────────
    {
        let page = page.clone();
        let s = (*search).clone();
        use_effect_with(s, move |_| { page.set(1); || () });
    }
    {
        let page = page.clone();
        let cf = (*col_filter_map).clone();
        use_effect_with(cf, move |_| { page.set(1); || () });
    }
    {
        let page = page.clone();
        let sc = *sort_col;
        let sd = (*sort_dir).clone();
        use_effect_with((sc, sd), move |_| { page.set(1); || () });
    }

    // ── Compute visible columns (in definition order) ─────────────────────────
    let vis_cols: Vec<&Column<R>> = props.columns.iter()
        .filter(|c| visible_cols.contains(c.key))
        .collect();

    // ── Client-side pipeline: filter → sort → paginate ────────────────────────
    let search_lower = search.to_lowercase();
    let mut processed: Vec<&R> = props.rows.iter().filter(|row| {
        // Global search across all visible columns
        if !search_lower.is_empty() {
            let hit = props.columns.iter().any(|col| {
                (col.search_value)(row).to_lowercase().contains(&search_lower)
            });
            if !hit { return false; }
        }
        // Per-column filters
        for (key, text) in col_filter_map.iter() {
            if text.is_empty() { continue; }
            let text_lower = text.to_lowercase();
            if let Some(col) = props.columns.iter().find(|c| c.key == key.as_str()) {
                if !(col.search_value)(row).to_lowercase().contains(&text_lower) {
                    return false;
                }
            }
        }
        true
    }).collect();

    // Sort
    if let Some(sort_key) = *sort_col {
        if let Some(col) = props.columns.iter().find(|c| c.key == sort_key) {
            if let Some(sv) = &col.sort_value {
                let dir = (*sort_dir).clone();
                processed.sort_by(|a, b| {
                    let ka = (sv)(a);
                    let kb = (sv)(b);
                    if dir == SortDir::Asc { ka.cmp(&kb) } else { kb.cmp(&ka) }
                });
            }
        }
    }

    // Pagination
    let total_filtered = processed.len();
    let ps = *page_size;
    let total_pages = ((total_filtered + ps - 1) / ps).max(1);
    let page_val = (*page).min(total_pages);
    let start = (page_val - 1) * ps;
    let end = (start + ps).min(total_filtered);
    let page_rows = &processed[start..end];

    // ── Column count (for colspan) ────────────────────────────────────────────
    let col_count = vis_cols.len()
        + 1  // row number
        + if props.row_actions.is_some() { 1 } else { 0 };

    // ── Callbacks ─────────────────────────────────────────────────────────────
    let on_search_input = {
        let search = search.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            search.set(el.value());
        })
    };

    let on_page_change = {
        let page = page.clone();
        Callback::from(move |p: usize| page.set(p))
    };
    let on_page_size_change = {
        let page = page.clone();
        let page_size = page_size.clone();
        Callback::from(move |s: usize| { page_size.set(s); page.set(1); })
    };

    let toggle_col_panel = {
        let show_col_panel = show_col_panel.clone();
        Callback::from(move |_: MouseEvent| show_col_panel.set(!*show_col_panel))
    };
    let toggle_filter_row = {
        let show_filter_row = show_filter_row.clone();
        Callback::from(move |_: MouseEvent| show_filter_row.set(!*show_filter_row))
    };

    // ── Header sort click ─────────────────────────────────────────────────────
    let make_sort_click = |key: &'static str| {
        let sort_col = sort_col.clone();
        let sort_dir = sort_dir.clone();
        Callback::from(move |_: MouseEvent| {
            if *sort_col == Some(key) {
                sort_dir.set(sort_dir.toggle());
            } else {
                sort_col.set(Some(key));
                sort_dir.set(SortDir::Asc);
            }
        })
    };

    // ── Header row ────────────────────────────────────────────────────────────
    let header_row = {
        let headers: Html = vis_cols.iter().enumerate().map(|(ci, col)| {
            let is_sorted = *sort_col == Some(col.key);
            let hov = if props.hoverable && *hovered_col == Some(ci + 1) { "col-hover" } else { "" };
            let style = col.width.map(|w| format!("width:{w};")).unwrap_or_default();
            if col.sortable {
                let onclick = make_sort_click(col.key);
                let icon = if is_sorted { sort_dir.icon() } else { "" };
                html! {
                    <th class={classes!("th-sortable", hov, is_sorted.then_some("th-sorted"))}
                        style={style}
                        {onclick}
                    >
                        { col.label }
                        <span class="sort-icon">{ icon }</span>
                    </th>
                }
            } else {
                html! { <th class={hov} style={style}>{ col.label }</th> }
            }
        }).collect();

        html! {
            <tr>
                <th class="th-row-num">{ "#" }</th>
                { headers }
                if props.row_actions.is_some() {
                    <th>{ "Actions" }</th>
                }
            </tr>
        }
    };

    // ── Filter row (in thead) ─────────────────────────────────────────────────
    let filter_row = if props.col_filters && *show_filter_row {
        let inputs: Html = vis_cols.iter().map(|col| {
            let key = col.key.to_string();
            let val = col_filter_map.get(&key).cloned().unwrap_or_default();
            let cfm = col_filter_map.clone();
            let k2 = key.clone();
            let oninput = Callback::from(move |e: InputEvent| {
                let el: HtmlInputElement = e.target_unchecked_into();
                let mut m = (*cfm).clone();
                m.insert(k2.clone(), el.value());
                cfm.set(m);
            });
            html! {
                <th class="dt-filter-th">
                    <input
                        type="text"
                        class="dt-filter-input"
                        placeholder="filter…"
                        value={val}
                        {oninput}
                    />
                </th>
            }
        }).collect();

        html! {
            <tr>
                <th class="dt-filter-th"></th>
                { inputs }
                if props.row_actions.is_some() {
                    <th class="dt-filter-th"></th>
                }
            </tr>
        }
    } else {
        html! {}
    };

    // ── Column toggle panel ───────────────────────────────────────────────────
    let col_panel = if props.col_toggle && *show_col_panel {
        let items: Html = props.columns.iter().map(|col| {
            let key = col.key.to_string();
            let checked = visible_cols.contains(col.key);
            let vc = visible_cols.clone();
            let k2 = key.clone();
            let onchange = Callback::from(move |e: Event| {
                let el: web_sys::HtmlInputElement = e.target_unchecked_into();
                let mut set = (*vc).clone();
                if el.checked() { set.insert(k2.clone()); } else { set.remove(&k2); }
                vc.set(set);
            });
            html! {
                <label class="dt-col-item">
                    <input type="checkbox" {checked} {onchange} />
                    { col.label }
                </label>
            }
        }).collect();
        html! { <div class="dt-col-panel">{ items }</div> }
    } else {
        html! {}
    };

    // ── Body rows ─────────────────────────────────────────────────────────────
    let body_rows: Html = if page_rows.is_empty() {
        html! {
            <tr>
                <td colspan={col_count.to_string()} class="no-data">
                    { props.empty_message.clone() }
                </td>
            </tr>
        }
    } else {
        page_rows.iter().enumerate().map(|(i, row)| {
            let key = (props.row_key)(row);
            let is_selected = selected_key.as_deref() == Some(&key);
            let global_i = start + i;

            // Row click: toggle selection
            let on_row_click = {
                let ski = selected_key_internal.clone();
                let on_select = props.on_select.clone();
                let k = key.clone();
                let sel = is_selected;
                Callback::from(move |_: MouseEvent| {
                    let new = if sel { None } else { Some(k.clone()) };
                    ski.set(new.clone());
                    if let Some(cb) = &on_select { cb.emit(new); }
                })
            };

            // Cells
            let cells: Html = vis_cols.iter().enumerate().map(|(ci, col)| {
                let hov = if props.hoverable && *hovered_col == Some(ci + 1) { "col-hover" } else { "" };
                let col_class = col.cell_class.unwrap_or("");
                let classes = classes!(col_class, hov);

                let on_enter = if props.hoverable {
                    let hc = hovered_col.clone();
                    let idx = ci + 1;
                    Some(Callback::from(move |_: MouseEvent| hc.set(Some(idx))))
                } else { None };
                let on_leave = if props.hoverable {
                    let hc = hovered_col.clone();
                    Some(Callback::from(move |_: MouseEvent| hc.set(None)))
                } else { None };

                html! {
                    <td class={classes} onmouseenter={on_enter} onmouseleave={on_leave}>
                        { (col.render)(row) }
                    </td>
                }
            }).collect();

            let detail_html = if is_selected {
                if let Some(detail_fn) = &props.row_detail {
                    html! {
                        <tr class="detail-row open">
                            <td colspan={col_count.to_string()}>
                                { detail_fn(row) }
                            </td>
                        </tr>
                    }
                } else { html! {} }
            } else { html! {} };

            let actions_html = if let Some(actions_fn) = &props.row_actions {
                html! { <td class="actions-col">{ actions_fn(row) }</td> }
            } else { html! {} };

            html! {
                <>
                    <tr
                        class={classes!("table-row", is_selected.then_some("selected-row"))}
                        onclick={on_row_click}
                        style="cursor:pointer;"
                    >
                        <td class="row-num">{ global_i + 1 }</td>
                        { cells }
                        { actions_html }
                    </tr>
                    { detail_html }
                </>
            }
        }).collect()
    };

    // ── Active filter count (for toolbar badge) ───────────────────────────────
    let active_filters = col_filter_map.values().filter(|v| !v.is_empty()).count();

    // ── Toolbar ───────────────────────────────────────────────────────────────
    let toolbar = html! {
        <div class="dt-toolbar">
            if props.searchable {
                <input
                    type="search"
                    class="dt-search"
                    placeholder="Search…"
                    value={(*search).clone()}
                    oninput={on_search_input}
                />
            }
            <span class="dt-toolbar-spacer" />
            if props.col_filters {
                <button
                    class={classes!("dt-toolbar-btn", (*show_filter_row).then_some("active"))}
                    onclick={toggle_filter_row}
                >
                    { if active_filters > 0 { format!("Filters ({})", active_filters) } else { "Filters".into() } }
                </button>
            }
            if props.col_toggle {
                <button
                    class={classes!("dt-toolbar-btn", (*show_col_panel).then_some("active"))}
                    onclick={toggle_col_panel}
                >
                    { "Columns" }
                </button>
            }
        </div>
    };

    // ── Page size options (default if not provided) ───────────────────────────
    let pso = if props.page_size_options.is_empty() {
        vec![10usize, 25, 50, 100]
    } else {
        props.page_size_options.clone()
    };

    html! {
        <div class="dt-root">
            { toolbar }
            { col_panel }
            <div class="table-wrapper">
                <div class="table-scroll">
                    <table class="trade-table">
                        <thead>
                            { header_row }
                            { filter_row }
                        </thead>
                        <tbody>
                            { body_rows }
                        </tbody>
                    </table>
                </div>
            </div>
            <Pagination
                current_page={page_val}
                total_pages={total_pages}
                total_items={total_filtered}
                page_size={ps}
                page_size_options={pso}
                on_page_change={on_page_change}
                on_page_size_change={on_page_size_change}
            />
        </div>
    }
}
