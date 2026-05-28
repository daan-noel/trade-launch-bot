use yew::prelude::*;

use crate::services::api::{TokenDetailRecord, TokenRecord};
use crate::state::{SortOrder, SortState, PriceUnitContext};
use crate::components::TokenRow;

#[derive(Properties, PartialEq)]
pub struct TokensTableProps {
    pub tokens: Vec<TokenRecord>,
    pub visible_cols: Vec<bool>,
    pub group_borders: Vec<bool>,
    pub num_cols: usize,
    pub sort: SortState,
    pub on_toggle_sort: Callback<String>,
    pub on_select_token: Callback<String>,
    pub hovered_column: Option<usize>,
    pub on_hover_column: Callback<Option<usize>>,
    pub selected_mint: Option<String>,
    pub selected_detail: Option<TokenDetailRecord>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    #[prop_or_default]
    pub offset: usize,
}

// Column configuration: (field, label, width_px, th_class)
pub const TOKENS_COLUMNS: &[(&str, &str, u32, &str)] = &[
    ("age", "Age", 60, "th-center"),
    ("last_trade", "Last Trade", 80, "th-center"),
    ("current_price", "Price", 100, "th-right"),
    ("market_cap", "MCap", 100, "th-right"),
    ("ath_price", "ATH Price", 100, "th-right"),
    ("ath_age", "ATH Age", 80, "th-center"),
    ("ath_fep", "ATH FEP", 80, "th-right"),
    ("current_fep", "Current FEP", 100, "th-right"),
    ("volume", "Volume", 100, "th-right"),
    ("init_supply", "Init Supply", 120, "th-right"),
    ("token_amount", "Token Amount", 120, "th-right"),
    ("max_sol_cost", "Max SOL Cost", 100, "th-right"),
    ("spendable_sol_in", "Spendable SOL In", 120, "th-right"),
    ("min_tokens_out", "Min Tokens Out", 120, "th-right"),
    ("trades", "Trades", 60, "th-center"),
    ("cu_limit", "CU Limit", 100, "th-right"),
    ("cu_price", "CU Price", 100, "th-right"),
    ("ix_count", "IX Count", 80, "th-center"),
    ("init_buy", "Init Buy", 80, "th-right"),
    ("migrated", "Migrated", 80, "th-center"),
];

#[function_component(TokensTable)]
pub fn tokens_table(props: &TokensTableProps) -> Html {
    let price_unit = use_context::<PriceUnitContext>()
        .expect("PriceUnitProvider must be mounted above TokensTable");

    let on_table_leave = {
        let on_hover_column = props.on_hover_column.clone();
        Callback::from(move |_: MouseEvent| on_hover_column.emit(None))
    };

    // Build headers — skip hidden columns
    let mut headers_html = vec![html! { <th class="th-row-num">{ "#" }</th> }];
    let mut rendered_col_pos = 1usize;

    for (i, &(field, label, _, th_cls)) in TOKENS_COLUMNS.iter().enumerate() {
        if !props.visible_cols[i] {
            continue;
        }
        let this_pos = rendered_col_pos;
        rendered_col_pos += 1;

        let is_col_hovered = props.hovered_column == Some(this_pos);
        let is_sorted = props.sort.field == field;
        let sort_icon = if is_sorted {
            match props.sort.order {
                SortOrder::Asc => "↑",
                SortOrder::Desc => "↓",
            }
        } else {
            ""
        };

        let on_click = {
            let on_toggle_sort = props.on_toggle_sort.clone();
            let field = field.to_string();
            Callback::from(move |_: MouseEvent| on_toggle_sort.emit(field.clone()))
        };

        let display_label = match field {
            "current_price" => format!("Price ({})", price_unit.unit_label()),
            "max_sol_cost" => "Max SOL Cost".to_string(),
            "spendable_sol_in" => "Spendable SOL In".to_string(),
            "market_cap" => format!("MCap ({})", price_unit.unit_label()),
            _ => label.to_string(),
        };

        let border_style = if props.group_borders[i] {
            "border-left: 1px solid rgba(128, 128, 128, 0.25);"
        } else {
            ""
        };

        headers_html.push(html! {
            <th class={classes!(th_cls, is_col_hovered.then_some("col-hover"))} style={border_style}>
                <button
                    class={classes!("sort-header-btn", is_sorted.then_some("sort-active"))}
                    onclick={on_click}
                    title={format!("Sort by {}", display_label)}
                >
                    { display_label }
                    { if is_sorted { html! { <span class="sort-icon">{ sort_icon }</span> } } else { html! {} } }
                </button>
            </th>
        });
    }
    headers_html.push(html! { <th class="th-action"></th> });

    let on_col_hover = {
        let on_hover_column = props.on_hover_column.clone();
        Callback::from(move |col: Option<usize>| on_hover_column.emit(col))
    };

    html! {
        <div class="table-wrapper">
            <table class="trade-table" onmouseleave={on_table_leave}>
                <colgroup>
                    <col style="width: 40px;" />
                    { for TOKENS_COLUMNS.iter().enumerate().filter_map(|(i, &(_, _, w, _))| {
                        if props.visible_cols[i] { Some(html! { <col style={format!("width: {}px;", w)} /> }) }
                        else { None }
                    }) }
                    <col style="width: 48px;" />
                </colgroup>
                <thead>
                    <tr>
                        { for headers_html }
                    </tr>
                </thead>
                <tbody>
                    { if props.tokens.is_empty() {
                            html! {
                            <tr>
                                <td class="no-data" colspan={props.num_cols.to_string()}>{ "No tokens found." }</td>
                            </tr>
                        }
                    } else {
                        html! {
                            { for props.tokens.iter().enumerate().map(|(idx, token)| {
                                let row_num = props.offset + idx + 1;
                                let selected = props.selected_mint.as_deref() == Some(&token.mint_address);
                                let detail = if selected { props.selected_detail.clone() } else { None };
                                html! {
                                    <TokenRow
                                        key={token.mint_address.clone()}
                                        token={token.clone()}
                                        selected={selected}
                                        detail={detail}
                                        detail_loading={props.detail_loading && selected}
                                        detail_error={props.detail_error.clone()}
                                        on_select={props.on_select_token.clone()}
                                        row_num={Some(row_num)}
                                        visible_cols={props.visible_cols.clone()}
                                        group_borders={props.group_borders.clone()}
                                        hovered_column={props.hovered_column}
                                        on_hover_column={on_col_hover.clone()}
                                    />
                                }
                            }) }
                        }
                    } }
                </tbody>
            </table>
        </div>
    }
}
