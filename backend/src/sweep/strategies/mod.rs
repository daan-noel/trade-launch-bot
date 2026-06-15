//! Per-strategy [`Strategy`](crate::sweep::strategy::Strategy) impls. Each module
//! owns one strategy's param set, axes, and pure `simulate`; the generic sweep
//! layers (corpus, grouping, engine, aggregate) never know which ran. Add a new
//! strategy by adding a module here and registering it in
//! [`registry`](crate::sweep::registry).

pub mod tpsl2;
