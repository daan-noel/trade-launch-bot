use serde::{Deserialize, Serialize};
use yew::prelude::*;

use crate::services::api::TokenRecord;

const LS_SORT_KEY: &str = "tokens_sort";

fn load_sort() -> SortState {
    let default = SortState::default();
    let window = match web_sys::window() {
        Some(w) => w,
        None => return default,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return default,
    };
    let raw = match storage.get_item(LS_SORT_KEY).ok().flatten() {
        Some(v) => v,
        None => return default,
    };
    serde_json::from_str(&raw).unwrap_or(default)
}

fn save_sort(sort: &SortState) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(s) => s,
        None => return,
    };
    if let Ok(json) = serde_json::to_string(sort) {
        let _ = storage.set_item(LS_SORT_KEY, &json);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SortState {
    pub field: String, // "symbol", "ath_price", "created", "volume", etc.
    pub order: SortOrder,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            field: "created".to_string(),
            order: SortOrder::Desc,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct TokenState {
    /// All tokens from current page
    pub tokens: Vec<TokenRecord>,
    pub total: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub sort: SortState,
    pub selected_mint: Option<String>,
}

impl Default for TokenState {
    fn default() -> Self {
        Self {
            tokens: Vec::new(),
            total: 0,
            loading: false,
            error: None,
            sort: SortState::default(),
            selected_mint: None,
        }
    }
}

#[allow(dead_code)]
pub enum TokenAction {
    SetLoading,
    SetTokens {
        tokens: Vec<TokenRecord>,
        total: usize,
    },
    /// Merge/replace tokens by `mint_address` so we can apply incremental updates
    /// UpdateTokens now accepts partial diffs so the client can apply SSE deltas
    UpdateTokens {
        diffs: Vec<TokenDiff>,
        total: Option<usize>,
    },
    SetError(String),
    ToggleSort(String), // field name - toggles order if same field, sets order to Desc if different
    SelectToken(String),
    ClearSelection,
}

pub fn sort_tokens(tokens: &mut Vec<TokenRecord>, sort: &SortState) {
    let field = sort.field.as_str();
    let order = sort.order;

    tokens.sort_by(|a, b| {
        let cmp = match field {
            "symbol" => a.symbol.cmp(&b.symbol),
            "first_entry_price" => {
                let price_a = a
                    .initial_buy_sol
                    .and_then(|buy| a.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                let price_b = b
                    .initial_buy_sol
                    .and_then(|buy| b.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                price_a
                    .partial_cmp(&price_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            "current_price" => a
                .current_price
                .partial_cmp(&b.current_price)
                .unwrap_or(std::cmp::Ordering::Equal),
            "ath_price" => a
                .ath_price
                .partial_cmp(&b.ath_price)
                .unwrap_or(std::cmp::Ordering::Equal),
            "ath_fep_ratio" | "fep_ath_ratio" => {
                let fep_a = a
                    .initial_buy_sol
                    .and_then(|buy| a.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                let fep_b = b
                    .initial_buy_sol
                    .and_then(|buy| b.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                let ratio_a = fep_a.and_then(|fep| {
                    a.ath_price
                        .and_then(|ath| if fep != 0.0 { Some(ath / fep) } else { None })
                });
                let ratio_b = fep_b.and_then(|fep| {
                    b.ath_price
                        .and_then(|ath| if fep != 0.0 { Some(ath / fep) } else { None })
                });
                ratio_a
                    .partial_cmp(&ratio_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            "current_fep_ratio" => {
                let fep_a = a
                    .initial_buy_sol
                    .and_then(|buy| a.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                let fep_b = b
                    .initial_buy_sol
                    .and_then(|buy| b.initial_supply_token.map(|supply| (buy, supply)))
                    .and_then(|(buy, supply)| {
                        if supply > 0 {
                            Some(buy / supply as f64)
                        } else {
                            None
                        }
                    });
                let ratio_a = fep_a.and_then(|fep| {
                    a.current_price
                        .and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
                });
                let ratio_b = fep_b.and_then(|fep| {
                    b.current_price
                        .and_then(|cur| if fep != 0.0 { Some(cur / fep) } else { None })
                });
                ratio_a
                    .partial_cmp(&ratio_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            "ath_timestamp" => a.ath_timestamp.cmp(&b.ath_timestamp),
            "volume" => a
                .volume_sol_total
                .partial_cmp(&b.volume_sol_total)
                .unwrap_or(std::cmp::Ordering::Equal),
            "market_cap" => a
                .market_cap
                .partial_cmp(&b.market_cap)
                .unwrap_or(std::cmp::Ordering::Equal),
            "initial_buy" => a
                .initial_buy_sol
                .partial_cmp(&b.initial_buy_sol)
                .unwrap_or(std::cmp::Ordering::Equal),
            "initial_supply" => a.initial_supply_token.cmp(&b.initial_supply_token),
            "token_amount" => a.token_amount.cmp(&b.token_amount),
            "max_sol_cost" => a.max_sol_cost.cmp(&b.max_sol_cost),
            "spendable_sol_in" => a.spendable_sol_in.cmp(&b.spendable_sol_in),
            "min_tokens_out" => a.min_tokens_out.cmp(&b.min_tokens_out),
            "cu_limit" => a.cu_limit.cmp(&b.cu_limit),
            "cu_price" => a.cu_price.cmp(&b.cu_price),
            "label_count" | "ix_count" => a.ix_labels_count.cmp(&b.ix_labels_count),
            "trade_count" => a.trade_count.cmp(&b.trade_count),
            "mayhem_mode" => a.is_mayhem_mode.cmp(&b.is_mayhem_mode),
            "migrated" => a.is_migrated.cmp(&b.is_migrated),
            "age" => a.age.cmp(&b.age),
            "name" => a.name.cmp(&b.name),
            "mint" => a.mint_address.cmp(&b.mint_address),
            "creator" => a.creator_address.cmp(&b.creator_address),
            "init_supply" => a.initial_supply_token.cmp(&b.initial_supply_token),
            "last_trade" => a.last_trade_at.cmp(&b.last_trade_at),
            "ix_labels" => a.ix_labels_count.cmp(&b.ix_labels_count),
            "create_tx" => a.create_tx_address.cmp(&b.create_tx_address),
            "created" | _ => a.created_at.cmp(&b.created_at),
        };

        match order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    });
}

#[derive(Clone, PartialEq, Debug)]
pub struct TokenDiff {
    pub mint_address: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub current_price: Option<f64>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<String>,
    pub market_cap: Option<f64>,
    /// Additive delta to volume (in SOL)
    pub volume_sol_delta: Option<f64>,
    /// Incremental trade count
    pub trade_count_delta: Option<u64>,
    pub last_trade_at: Option<String>,
    pub initial_buy_sol: Option<f64>,
    pub initial_supply_token: Option<u64>,
    pub cu_limit: Option<u64>,
    pub cu_price: Option<u64>,
    pub is_migrated: Option<bool>,
}

impl Reducible for TokenState {
    type Action = TokenAction;

    fn reduce(self: std::rc::Rc<Self>, action: Self::Action) -> std::rc::Rc<Self> {
        let mut next = (*self).clone();
        match action {
            TokenAction::SetLoading => next.loading = true,
            TokenAction::SetTokens { mut tokens, total } => {
                // Sort with current sort preferences
                sort_tokens(&mut tokens, &next.sort);
                next.tokens = tokens;
                next.total = total;
                next.loading = false;
                next.error = None;
            }
            TokenAction::UpdateTokens { diffs, total } => {
                for diff in diffs.into_iter() {
                    let mint = diff.mint_address.clone();
                    if let Some(pos) = next.tokens.iter().position(|t| t.mint_address == mint) {
                        let t = &mut next.tokens[pos];
                        if let Some(name) = diff.name {
                            t.name = name
                        }
                        if let Some(sym) = diff.symbol {
                            t.symbol = sym
                        }
                        if let Some(cp) = diff.current_price {
                            t.current_price = Some(cp)
                        }
                        if let Some(ath) = diff.ath_price {
                            t.ath_price = Some(ath)
                        }
                        if let Some(ath_ts) = diff.ath_timestamp {
                            t.ath_timestamp = Some(ath_ts)
                        }
                        if let Some(mc) = diff.market_cap {
                            t.market_cap = Some(mc)
                        }
                        if let Some(delta) = diff.volume_sol_delta {
                            t.volume_sol_total = t.volume_sol_total + delta
                        }
                        if let Some(dn) = diff.trade_count_delta {
                            t.trade_count = t.trade_count.saturating_add(dn)
                        }
                        if let Some(last) = diff.last_trade_at {
                            t.last_trade_at = Some(last)
                        }
                        if diff.initial_buy_sol.is_some() {
                            t.initial_buy_sol = diff.initial_buy_sol
                        }
                        if diff.initial_supply_token.is_some() {
                            t.initial_supply_token = diff.initial_supply_token
                        }
                        if diff.cu_limit.is_some() {
                            t.cu_limit = diff.cu_limit
                        }
                        if diff.cu_price.is_some() {
                            t.cu_price = diff.cu_price
                        }
                        if let Some(mig) = diff.is_migrated {
                            t.is_migrated = mig
                        }
                    } else {
                        // Do not add new tokens to the current page state. The list
                        // is a paginated/search result and should only update rows
                        // that are already present.
                    }
                }
                if let Some(t) = total {
                    next.total = t
                }
                sort_tokens(&mut next.tokens, &next.sort);
                next.loading = false;
                next.error = None;
            }
            TokenAction::SetError(err) => {
                next.error = Some(err);
                next.loading = false;
            }
            TokenAction::ToggleSort(field) => {
                if next.sort.field == field {
                    next.sort.order = next.sort.order.toggle();
                } else {
                    next.sort.field = field;
                    next.sort.order = SortOrder::Desc;
                }
                save_sort(&next.sort);
                sort_tokens(&mut next.tokens, &next.sort);
            }
            TokenAction::SelectToken(mint) => {
                if next.selected_mint.as_deref() == Some(&mint) {
                    next.selected_mint = None;
                } else {
                    next.selected_mint = Some(mint);
                }
            }
            TokenAction::ClearSelection => {
                next.selected_mint = None;
            }
        }
        next.into()
    }
}

pub type TokenContext = UseReducerHandle<TokenState>;

#[derive(Properties, PartialEq)]
pub struct TokenProviderProps {
    pub children: Children,
}

#[function_component(TokenProvider)]
pub fn token_provider(props: &TokenProviderProps) -> Html {
    let state = use_reducer_eq(|| TokenState {
        sort: load_sort(),
        ..TokenState::default()
    });

    html! {
        <ContextProvider<TokenContext> context={state}>
            { for props.children.iter() }
        </ContextProvider<TokenContext>>
    }
}
