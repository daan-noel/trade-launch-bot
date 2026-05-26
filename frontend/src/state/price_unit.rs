use serde::{Deserialize, Serialize};
use web_sys;
use yew::prelude::*;

use crate::utils::format::{format_compact, format_decimal, format_price, format_with_commas};

const LS_PRICE_UNIT_KEY: &str = "price_unit";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PriceUnit {
    SOL,
    USD,
}

impl Default for PriceUnit {
    fn default() -> Self {
        Self::SOL
    }
}

impl PriceUnit {
    pub fn label(self) -> &'static str {
        match self {
            PriceUnit::SOL => "SOL",
            PriceUnit::USD => "USD",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceUnitState {
    pub unit: PriceUnit,
    pub usd_rate: Option<f64>,
}

impl Default for PriceUnitState {
    fn default() -> Self {
        Self {
            unit: PriceUnit::SOL,
            usd_rate: None,
        }
    }
}

pub enum PriceUnitAction {
    SetUnit(PriceUnit),
    SetUsdRate(Option<f64>),
}

impl Reducible for PriceUnitState {
    type Action = PriceUnitAction;

    fn reduce(self: std::rc::Rc<Self>, action: Self::Action) -> std::rc::Rc<Self> {
        let mut next = (*self).clone();
        match action {
            PriceUnitAction::SetUnit(unit) => next.unit = unit,
            PriceUnitAction::SetUsdRate(rate) => next.usd_rate = rate,
        }
        save_price_unit(&next);
        next.into()
    }
}

pub type PriceUnitContext = UseReducerHandle<PriceUnitState>;

#[derive(Properties, PartialEq)]
pub struct PriceUnitProviderProps {
    pub children: Children,
}

#[function_component(PriceUnitProvider)]
pub fn price_unit_provider(props: &PriceUnitProviderProps) -> Html {
    let reducer = use_reducer_eq(load_price_unit);

    html! {
        <ContextProvider<PriceUnitContext> context={reducer}>
            { for props.children.iter() }
        </ContextProvider<PriceUnitContext>>
    }
}

impl PriceUnitState {
    pub fn display_price(&self, sol_value: f64) -> String {
        match self.unit {
            PriceUnit::SOL => format_price(sol_value),
            PriceUnit::USD => self
                .usd_rate
                .map(|rate| format_usd(sol_value * rate))
                .unwrap_or_else(|| format_price(sol_value)),
        }
    }

    pub fn display_amount(&self, sol_value: f64) -> String {
        match self.unit {
            PriceUnit::SOL => format_decimal(sol_value, 4),
            PriceUnit::USD => self
                .usd_rate
                .map(format_usd)
                .unwrap_or_else(|| format_decimal(sol_value, 4)),
        }
    }

    pub fn display_compact(&self, sol_value: f64, digits: usize) -> String {
        match self.unit {
            PriceUnit::SOL => format_compact(sol_value, digits),
            PriceUnit::USD => self
                .usd_rate
                .map(format_usd)
                .unwrap_or_else(|| format_compact(sol_value, digits)),
        }
    }

    pub fn unit_label(&self) -> &'static str {
        self.unit.label()
    }
}

fn load_price_unit() -> PriceUnitState {
    let window = match web_sys::window() {
        Some(window) => window,
        None => return PriceUnitState::default(),
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(storage) => storage,
        None => return PriceUnitState::default(),
    };
    match storage.get_item(LS_PRICE_UNIT_KEY).ok().flatten() {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        None => PriceUnitState::default(),
    }
}

fn save_price_unit(state: &PriceUnitState) {
    let window = match web_sys::window() {
        Some(window) => window,
        None => return,
    };
    let storage = match window.local_storage().ok().flatten() {
        Some(storage) => storage,
        None => return,
    };

    if let Ok(raw) = serde_json::to_string(state) {
        let _ = storage.set_item(LS_PRICE_UNIT_KEY, &raw);
    }
}

fn format_usd(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let rounded = (value.abs() * 100.0).round() / 100.0;
    let whole = rounded.trunc() as u64;
    let frac = ((rounded - whole as f64) * 100.0).round() as u64;
    let whole_str = format_with_commas(whole);
    if frac == 0 {
        format!("{sign}${whole_str}")
    } else {
        format!("{sign}${whole_str}.{:02}", frac)
    }
}
