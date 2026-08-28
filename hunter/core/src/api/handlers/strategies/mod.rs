//! Core strategy handlers. The generic-engine rule **domain** layer lives in
//! [`trading_core::strategies::rules`] (validate → build `StrategyRule` → `RuleRepo`
//! write). The live position/lifecycle handlers live in `live`; the simulate/
//! paper-result handlers live in `lab`. The legacy per-strategy tpsl1/tpsl2/swing1
//! rule stack does not exist; there is one generic engine.
//!
//! Per-rule position **reads** are the exception: [`rule_positions`] holds the one
//! implementation both bins serve (live off its own table, lab off the synced
//! mirror), so the run-scope semantics + wire shape can't drift between them.
//!
//! [`rule_bundle`] is shared for the same reason and one more: it moves rules
//! BETWEEN the two boxes, so a diff computed differently on either side would
//! approve one change and apply another.

pub mod rule_bundle;
pub mod rule_positions;
