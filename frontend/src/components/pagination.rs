use web_sys::HtmlSelectElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct PaginationProps {
    pub current_page: usize,
    pub total_pages: usize,
    pub total_items: usize,
    pub page_size: usize,
    pub page_size_options: Vec<usize>,
    pub on_page_change: Callback<usize>,
    pub on_page_size_change: Callback<usize>,
}

fn build_page_buttons(current_page: usize, total_pages: usize) -> Vec<usize> {
    let mut buttons = Vec::new();

    if total_pages <= 9 {
        for page_index in 1..=total_pages {
            buttons.push(page_index);
        }
    } else if current_page <= 5 {
        for page_index in 1..=6 {
            buttons.push(page_index);
        }
        buttons.push(0);
        buttons.push(total_pages);
    } else if current_page + 4 >= total_pages {
        buttons.push(1);
        buttons.push(0);
        for page_index in (total_pages.saturating_sub(5))..=total_pages {
            buttons.push(page_index);
        }
    } else {
        buttons.push(1);
        buttons.push(0);
        for page_index in (current_page.saturating_sub(1))..=(current_page + 1) {
            buttons.push(page_index);
        }
        buttons.push(0);
        buttons.push(total_pages);
    }

    buttons
}

#[function_component(Pagination)]
pub fn pagination(props: &PaginationProps) -> Html {
    let on_prev = {
        let on_page_change = props.on_page_change.clone();
        let current_page = props.current_page;
        Callback::from(move |_: MouseEvent| {
            if current_page > 1 {
                on_page_change.emit(current_page - 1);
            }
        })
    };

    let on_next = {
        let on_page_change = props.on_page_change.clone();
        let current_page = props.current_page;
        let total_pages = props.total_pages;
        Callback::from(move |_: MouseEvent| {
            if current_page < total_pages {
                on_page_change.emit(current_page + 1);
            }
        })
    };

    let on_page_size_change = {
        let on_page_size_change = props.on_page_size_change.clone();
        Callback::from(move |e: Event| {
            let el: HtmlSelectElement = e.target_unchecked_into();
            if let Ok(value) = el.value().parse::<usize>() {
                on_page_size_change.emit(value);
            }
        })
    };

    let page_buttons = build_page_buttons(props.current_page, props.total_pages);

    html! {
        <div class="pagination-component">
            <span class="pagination-summary">
                { format!("Page {} of {} • {} total", props.current_page, props.total_pages, props.total_items) }
            </span>

            <div class="pagination-controls">
                <button
                    class={classes!("page-nav", (props.current_page <= 1).then_some("disabled"))}
                    onclick={on_prev}
                    disabled={props.current_page <= 1}
                >
                    { "‹" }
                </button>
                { for page_buttons.into_iter().map(|page_index| {
                    if page_index == 0 {
                        html! { <span class="page-ellipsis">{"..."}</span> }
                    } else {
                        let page_index_copy = page_index;
                        let is_active = page_index_copy == props.current_page;
                        let on_page_change = props.on_page_change.clone();
                        html! {
                            <button
                                class={classes!("page-button", is_active.then_some("active"))}
                                onclick={Callback::from(move |_: MouseEvent| on_page_change.emit(page_index_copy))}
                                disabled={is_active}
                            >
                                { page_index_copy }
                            </button>
                        }
                    }
                }) }
                <button
                    class={classes!("page-nav", (props.current_page >= props.total_pages).then_some("disabled"))}
                    onclick={on_next}
                    disabled={props.current_page >= props.total_pages}
                >
                    { "›" }
                </button>
            </div>

            <div class="page-size-label">
                { "Show " }
                <select onchange={on_page_size_change}>
                    { for props.page_size_options.iter().map(|&size| html! {
                        <option value={size.to_string()} selected={size == props.page_size}>
                            { size }
                        </option>
                    }) }
                </select>
            </div>
        </div>
    }
}
