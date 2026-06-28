//! Core strategy handlers / domain. Holds the trading-free rule **domain** layer
//! (`tpsl_rules_core`: request DTOs + validation + repo write) shared by both the
//! deploy CRUD edge (adds the live cache reload) and the local CRUD edge (adds
//! nothing). The live position/lifecycle handlers live in `live`; the
//! simulate/paper-result handlers live in `lab`.

pub mod tpsl_rules_core;
