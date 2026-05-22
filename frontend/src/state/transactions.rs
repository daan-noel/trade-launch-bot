use serde::{Deserialize, Serialize};
use yew::prelude::*;

/// Matches the `trade_executed` SSE event data sent by the backend.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct LiveTrade {
    pub mint: String,
    pub wallet: String,
    pub trade_type: String, // "buy" | "sell"
    pub sol_amount: f64,
    pub token_amount: f64,
    pub price_per_token: f64,
    pub tx_signature: String,
    pub slot: u64,
    pub timestamp: String, // ISO 8601
}

#[derive(Clone, PartialEq)]
pub struct TransactionState {
    /// Live trades received from SSE, newest first.  Capped at 500.
    pub events: Vec<LiveTrade>,
}

impl Default for TransactionState {
    fn default() -> Self {
        Self { events: Vec::new() }
    }
}

#[allow(dead_code)]
pub enum TransactionAction {
    Prepend(LiveTrade),
    Clear,
}

impl Reducible for TransactionState {
    type Action = TransactionAction;

    fn reduce(self: std::rc::Rc<Self>, action: Self::Action) -> std::rc::Rc<Self> {
        let mut next = (*self).clone();
        match action {
            TransactionAction::Prepend(ev) => {
                if next.events.len() >= 500 {
                    next.events.pop();
                }
                next.events.insert(0, ev);
            }
            TransactionAction::Clear => next.events.clear(),
        }
        next.into()
    }
}
