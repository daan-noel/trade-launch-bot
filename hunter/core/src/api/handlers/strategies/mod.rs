//! Core strategy handlers. The rule **domain** layer now lives in
//! [`trading_core::strategies::rules`] (validate → build `StrategyRule` → unified
//! `strategy_repo` write), shared by both the deploy CRUD edge (adds the live cache
//! reload + `rules_changed` SSE) and the local CRUD edge (adds nothing). The live
//! position/lifecycle handlers live in `live`; the simulate/paper-result handlers
//! live in `lab`. The legacy `tpsl_rules_core` module was removed with the
//! strategy-table merge.
