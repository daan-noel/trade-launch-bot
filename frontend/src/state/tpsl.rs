use crate::services::api::{RulePositionRecord, RuleRecord};
use yew::prelude::*;

#[derive(Clone, PartialEq, Default)]
pub struct TpslState {
    pub rules: Vec<RuleRecord>,
    pub loading: bool,
    pub error: Option<String>,
    pub positions: Vec<RulePositionRecord>,
    pub positions_loading: bool,
    pub positions_error: Option<String>,
}

pub enum TpslAction {
    SetLoading,
    SetRules(Vec<RuleRecord>),
    SetError(String),
    SetPositionsLoading,
    SetPositions(Vec<RulePositionRecord>),
    SetPositionsError(String),
    ClearPositions,
    AddRule(RuleRecord),
    UpdateRule(RuleRecord),
    RemoveRule(String),
}

impl Reducible for TpslState {
    type Action = TpslAction;

    fn reduce(self: std::rc::Rc<Self>, action: Self::Action) -> std::rc::Rc<Self> {
        let mut next = (*self).clone();
        match action {
            TpslAction::SetLoading => {
                next.loading = true;
            }
            TpslAction::SetRules(rules) => {
                next.rules = rules;
                next.loading = false;
                next.error = None;
            }
            TpslAction::SetError(err) => {
                next.error = Some(err);
                next.loading = false;
            }
            TpslAction::SetPositionsLoading => {
                next.positions_loading = true;
                next.positions_error = None;
            }
            TpslAction::SetPositions(positions) => {
                next.positions = positions;
                next.positions_loading = false;
                next.positions_error = None;
            }
            TpslAction::SetPositionsError(err) => {
                next.positions_error = Some(err);
                next.positions_loading = false;
            }
            TpslAction::ClearPositions => {
                next.positions = vec![];
                next.positions_error = None;
                next.positions_loading = false;
            }
            TpslAction::AddRule(rule) => {
                next.rules.insert(0, rule);
            }
            TpslAction::UpdateRule(updated) => {
                if let Some(r) = next.rules.iter_mut().find(|r| r.id == updated.id) {
                    *r = updated;
                }
            }
            TpslAction::RemoveRule(id) => {
                next.rules.retain(|r| r.id != id);
            }
        }
        next.into()
    }
}

pub type TpslContext = UseReducerHandle<TpslState>;

#[derive(Properties, PartialEq)]
pub struct TpslProviderProps {
    pub children: Children,
}

#[function_component(TpslProvider)]
pub fn tpsl_provider(props: &TpslProviderProps) -> Html {
    let state = use_reducer_eq(TpslState::default);
    html! {
        <ContextProvider<TpslContext> context={state}>
            { for props.children.iter() }
        </ContextProvider<TpslContext>>
    }
}
