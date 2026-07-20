//! Core strategy handlers. The generic-engine rule **domain** layer lives in
//! [`trading_core::strategies::rules`] (validate → build `StrategyRule` → `RuleRepo`
//! write). The live position/lifecycle handlers live in `live`; the simulate/
//! paper-result handlers live in `lab`. The legacy per-strategy tpsl1/tpsl2/swing1
//! rule stack was retired in Phase 7.
